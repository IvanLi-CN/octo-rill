use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use syn::{
    Attribute, Expr, ExprCall, ExprMethodCall, File, Item, ItemFn, ItemMod, Meta,
    spanned::Spanned,
    visit::{self, Visit},
};
use walkdir::WalkDir;

#[derive(Debug, Deserialize)]
struct QualityConfig {
    #[serde(default)]
    legacy_suppressions: BTreeMap<String, usize>,
}

#[derive(Default)]
struct SourceVisitor {
    path: String,
    suppressions: Vec<String>,
    structural_issues: Vec<String>,
    sqlite_write_issues: Vec<String>,
    source_lines: Vec<String>,
    test_only_depth: usize,
    sqlite_writer_closure_depth: usize,
}

impl<'ast> Visit<'ast> for SourceVisitor {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.inspect_attribute(attribute);
        visit::visit_attribute(self, attribute);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let previous_test_only_depth = self.test_only_depth;
        if has_cfg_test(&item.attrs) {
            self.test_only_depth += 1;
        }
        visit::visit_item_fn(self, item);
        self.test_only_depth = previous_test_only_depth;
    }

    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        let previous_test_only_depth = self.test_only_depth;
        if has_cfg_test(&item.attrs) {
            self.test_only_depth += 1;
        }
        visit::visit_item_mod(self, item);
        self.test_only_depth = previous_test_only_depth;
    }

    fn visit_expr_method_call(&mut self, expression: &'ast ExprMethodCall) {
        let method = expression.method.to_string();
        let is_sqlite_writer_call = is_sqlite_writer_expression(&expression.receiver)
            && matches!(
                method.as_str(),
                "write" | "write_foreground" | "write_with_priority" | "try_write"
            );

        if self.test_only_depth == 0
            && is_direct_pool_write(expression)
            && self.sqlite_writer_closure_depth == 0
            && !self.has_sqlite_guard_exception(expression)
        {
            self.sqlite_write_issues.push(format!(
                "{}:{}: direct SQLite pool write must use SqliteWriteCoordinator",
                self.path,
                expression.span().start().line
            ));
        }

        if is_sqlite_writer_call {
            visit::visit_expr(&mut *self, &expression.receiver);
            for argument in &expression.args {
                let is_closure = matches!(argument, Expr::Closure(_));
                if is_closure {
                    self.sqlite_writer_closure_depth += 1;
                }
                visit::visit_expr(&mut *self, argument);
                if is_closure {
                    self.sqlite_writer_closure_depth =
                        self.sqlite_writer_closure_depth.saturating_sub(1);
                }
            }
            return;
        }

        visit::visit_expr_method_call(self, expression);
    }

    fn visit_expr_call(&mut self, expression: &'ast ExprCall) {
        let is_sqlite_writer_wrapper = matches!(
            &*expression.func,
            Expr::Path(path)
                if path
                    .path
                    .segments
                    .last()
                    .is_some_and(|segment| segment.ident == "run_subscription_prune_phase")
        );

        if is_sqlite_writer_wrapper {
            visit::visit_expr(self, &expression.func);
            for argument in &expression.args {
                let is_closure = matches!(argument, Expr::Closure(_));
                if is_closure {
                    self.sqlite_writer_closure_depth += 1;
                }
                visit::visit_expr(self, argument);
                if is_closure {
                    self.sqlite_writer_closure_depth =
                        self.sqlite_writer_closure_depth.saturating_sub(1);
                }
            }
            return;
        }

        visit::visit_expr_call(self, expression);
    }
}

impl SourceVisitor {
    fn inspect_attribute(&mut self, attribute: &Attribute) {
        let Some(attribute_name) = attribute.path().get_ident().map(ToString::to_string) else {
            return;
        };

        let contains_suppression = match &attribute.meta {
            Meta::List(list) => {
                let tokens = list.tokens.to_string();
                tokens.contains("allow") || tokens.contains("expect")
            }
            _ => false,
        };
        if matches!(attribute.style, syn::AttrStyle::Inner(_))
            && matches!(attribute_name.as_str(), "allow" | "expect" | "cfg_attr")
            && contains_suppression
        {
            self.structural_issues.push(format!(
                "{}: inner structural suppression is not permitted ({attribute_name})",
                self.path
            ));
        }

        match attribute_name.as_str() {
            "allow" | "expect" => self.record_direct_suppressions(attribute, &attribute_name),
            "cfg_attr" => self.record_cfg_attr_suppressions(attribute),
            _ => {}
        }
    }

    fn record_direct_suppressions(&mut self, attribute: &Attribute, kind: &str) {
        let Meta::List(list) = &attribute.meta else {
            self.structural_issues
                .push(format!("{}: {} must use a lint list", self.path, kind));
            return;
        };

        let parse_result = list.parse_nested_meta(|nested| {
            self.suppressions.push(format!(
                "{}|{}|{}",
                self.path,
                kind,
                path_name(&nested.path)
            ));
            Ok(())
        });
        if let Err(error) = parse_result {
            self.structural_issues.push(format!(
                "{}: could not parse {} attribute: {}",
                self.path, kind, error
            ));
        }
    }

    fn record_cfg_attr_suppressions(&mut self, attribute: &Attribute) {
        let Meta::List(list) = &attribute.meta else {
            return;
        };
        let rendered = list
            .tokens
            .to_string()
            .chars()
            .filter(|character| !character.is_whitespace())
            .collect::<String>();

        for kind in ["allow", "expect"] {
            let marker = format!("{kind}(");
            let Some(start) = rendered.find(&marker) else {
                continue;
            };
            let remainder = &rendered[start + marker.len()..];
            let lint = remainder.split(')').next().unwrap_or_default();
            if !lint.is_empty() {
                self.suppressions
                    .push(format!("{}|{}|{}", self.path, kind, lint));
            }
        }
    }
}

fn path_name(path: &syn::Path) -> String {
    path.segments
        .iter()
        .map(|segment| segment.ident.to_string())
        .collect::<Vec<_>>()
        .join("::")
}

const SQLITE_WRITE_GUARD_PATHS: [&str; 6] = [
    "src/admin_runtime.rs",
    "src/ai.rs",
    "src/api.rs",
    "src/jobs.rs",
    "src/sync.rs",
    "src/translations.rs",
];

fn has_cfg_test(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        let Meta::List(list) = &attribute.meta else {
            return false;
        };
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        let Ok(predicates) = list
            .parse_args_with(syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated)
        else {
            return false;
        };
        predicates.iter().any(cfg_predicate_contains_test)
    })
}

fn cfg_predicate_contains_test(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") || list.path.is_ident("any") => {
            let Ok(predicates) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            predicates.iter().any(cfg_predicate_contains_test)
        }
        Meta::List(_) | Meta::NameValue(_) => false,
    }
}

fn is_sqlite_writer_expression(expression: &Expr) -> bool {
    match expression {
        Expr::Field(field) => {
            matches!(&field.member, syn::Member::Named(member) if member == "sqlite_writer")
                || is_sqlite_writer_expression(&field.base)
        }
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "sqlite_writer"),
        Expr::Reference(reference) => is_sqlite_writer_expression(&reference.expr),
        Expr::Paren(paren) => is_sqlite_writer_expression(&paren.expr),
        _ => false,
    }
}

fn expression_mentions_pool(expression: &Expr) -> bool {
    match expression {
        Expr::Field(field) => {
            matches!(&field.member, syn::Member::Named(member) if member == "pool")
                || expression_mentions_pool(&field.base)
        }
        Expr::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "pool"),
        Expr::Reference(reference) => expression_mentions_pool(&reference.expr),
        Expr::Paren(paren) => expression_mentions_pool(&paren.expr),
        _ => false,
    }
}

fn is_direct_pool_write(expression: &ExprMethodCall) -> bool {
    if expression.method == "execute" {
        return expression.args.last().is_some_and(expression_mentions_pool);
    }

    expression.method == "begin_with"
        && expression_mentions_pool(&expression.receiver)
        && expression.args.first().is_some_and(|argument| {
            matches!(
                argument,
                Expr::Lit(literal)
                    if matches!(&literal.lit, syn::Lit::Str(value) if value.value().contains("BEGIN IMMEDIATE"))
            )
        })
}

impl SourceVisitor {
    fn has_sqlite_guard_exception(&self, expression: &ExprMethodCall) -> bool {
        let line_index = expression.span().start().line.saturating_sub(1);
        self.source_lines
            .get(line_index.saturating_sub(1))
            .is_some_and(|line| line.contains("sqlite-write-guard: allow-direct-pool-write"))
    }
}

fn validate_main_entrypoint(file: &File) -> Vec<String> {
    let mut issues = Vec::new();
    for item in &file.items {
        let allowed = match item {
            Item::Mod(_) | Item::Use(_) => true,
            Item::Fn(function) => function.sig.ident == "main",
            _ => false,
        };
        if !allowed {
            issues.push(
                "src/main.rs: entrypoint may only contain module declarations, imports, and main()"
                    .to_owned(),
            );
        }
    }
    issues
}

fn scan_source(path: &Path, source: &str) -> Result<SourceVisitor, String> {
    let relative_path = path.to_string_lossy().replace('\\', "/");
    let file = syn::parse_file(source)
        .map_err(|error| format!("{relative_path}: Rust source parse failed: {error}"))?;
    let mut visitor = SourceVisitor {
        path: relative_path.clone(),
        source_lines: source.lines().map(str::to_owned).collect(),
        ..SourceVisitor::default()
    };
    visitor.visit_file(&file);
    if SQLITE_WRITE_GUARD_PATHS.contains(&relative_path.as_str()) {
        visitor
            .structural_issues
            .extend(visitor.sqlite_write_issues.clone());
    }
    if relative_path == "src/main.rs" {
        visitor
            .structural_issues
            .extend(validate_main_entrypoint(&file));
    }
    Ok(visitor)
}

fn load_config(root: &Path) -> Result<QualityConfig, String> {
    let path = root.join("rust-source-quality.toml");
    let content = fs::read_to_string(&path)
        .map_err(|error| format!("{}: could not read config: {error}", path.display()))?;
    toml::from_str(&content)
        .map_err(|error| format!("{}: could not parse config: {error}", path.display()))
}

fn collect_rust_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let source_root = root.join("src");
    let mut paths = Vec::new();
    for entry in WalkDir::new(&source_root) {
        let entry =
            entry.map_err(|error| format!("could not walk {}: {error}", source_root.display()))?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|ext| ext == "rs") {
            paths.push(entry.into_path());
        }
    }
    paths.sort();
    Ok(paths)
}

fn run(root: &Path) -> Result<(), String> {
    let config = load_config(root)?;
    let mut observed = BTreeMap::<String, usize>::new();
    let mut issues = Vec::new();
    let source_paths = collect_rust_sources(root)?;

    for path in &source_paths {
        let source = fs::read_to_string(path)
            .map_err(|error| format!("{}: could not read source: {error}", path.display()))?;
        let relative_path = path.strip_prefix(root).map_err(|error| {
            format!(
                "{}: could not relativize source path: {error}",
                path.display()
            )
        })?;
        let visitor = scan_source(relative_path, &source)?;
        for issue in visitor.structural_issues {
            issues.push(issue);
        }
        for suppression in visitor.suppressions {
            *observed.entry(suppression).or_default() += 1;
        }
    }

    for (suppression, count) in &observed {
        let allowed = config
            .legacy_suppressions
            .get(suppression)
            .copied()
            .unwrap_or(0);
        if *count > allowed {
            issues.push(format!(
                "{suppression}: observed {count} occurrence(s), baseline allows {allowed}; add a narrow reviewed waiver or remove the suppression"
            ));
        }
    }

    if issues.is_empty() {
        println!(
            "rust-source-quality: checked {} Rust source files and {} legacy suppression entries",
            source_paths.len(),
            observed.values().sum::<usize>()
        );
        return Ok(());
    }

    issues.sort();
    Err(issues.join("\n"))
}

fn main() {
    let root = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let root = match fs::canonicalize(&root) {
        Ok(root) => root,
        Err(error) => {
            eprintln!(
                "rust-source-quality: could not resolve {}: {error}",
                root.display()
            );
            std::process::exit(1);
        }
    };

    if let Err(error) = run(&root) {
        eprintln!("rust-source-quality: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entrypoint_rejects_domain_items() {
        let file = syn::parse_file("mod feature; fn main() {} const VALUE: u8 = 1;")
            .expect("test source should parse");
        assert_eq!(validate_main_entrypoint(&file).len(), 1);
    }

    #[test]
    fn path_name_preserves_clippy_namespace() {
        let path: syn::Path =
            syn::parse_str("clippy::too_many_arguments").expect("path should parse");
        assert_eq!(path_name(&path), "clippy::too_many_arguments");
    }

    #[test]
    fn sqlite_write_guard_rejects_uncoordinated_pool_writes() {
        let source = r#"
            async fn persist(pool: &SqlitePool) {
                sqlx::query("UPDATE settings SET value = 1")
                    .execute(pool)
                    .await
                    .unwrap();
            }
        "#;
        let visitor = scan_source(Path::new("src/admin_runtime.rs"), source)
            .expect("test source should parse");
        assert_eq!(visitor.sqlite_write_issues.len(), 1);
    }

    #[test]
    fn sqlite_write_guard_accepts_coordinator_pool_writes() {
        let source = r#"
            async fn persist(pool: &SqlitePool, sqlite_writer: &SqliteWriteCoordinator) {
                sqlite_writer
                    .write("settings_update", |_| async {
                        sqlx::query("UPDATE settings SET value = 1")
                            .execute(pool)
                            .await
                            .unwrap();
                        Ok(())
                    })
                    .await
                    .unwrap();
            }
        "#;
        let visitor = scan_source(Path::new("src/admin_runtime.rs"), source)
            .expect("test source should parse");
        assert!(visitor.sqlite_write_issues.is_empty());
    }

    #[test]
    fn sqlite_write_guard_accepts_known_coordinator_wrapper_callbacks() {
        let source = r#"
            async fn persist(state: &AppState) {
                run_subscription_prune_phase(
                    state,
                    "prune",
                    "prune",
                    "busy",
                    "busy",
                    || async {
                        sqlx::query("DELETE FROM settings")
                            .execute(&state.pool)
                            .await
                            .map(|result| result.rows_affected())
                            .map_err(anyhow::Error::from)
                    },
                )
                .await
                .unwrap();
            }
        "#;
        let visitor =
            scan_source(Path::new("src/sync.rs"), source).expect("test source should parse");
        assert!(visitor.sqlite_write_issues.is_empty());
    }

    #[test]
    fn sqlite_write_guard_allows_test_only_pool_writes() {
        let source = r#"
            #[cfg(test)]
            mod tests {
                async fn persist(pool: &SqlitePool) {
                    sqlx::query("UPDATE settings SET value = 1")
                        .execute(pool)
                        .await
                        .unwrap();
                }
            }
        "#;
        let visitor = scan_source(Path::new("src/admin_runtime.rs"), source)
            .expect("test source should parse");
        assert!(visitor.sqlite_write_issues.is_empty());
    }

    #[test]
    fn sqlite_write_guard_does_not_treat_cfg_not_test_as_test() {
        let source = r#"
            #[cfg(not(test))]
            mod production {
                async fn persist(pool: &SqlitePool) {
                    sqlx::query("UPDATE settings SET value = 1")
                        .execute(pool)
                        .await
                        .unwrap();
                }
            }
        "#;
        let visitor = scan_source(Path::new("src/admin_runtime.rs"), source)
            .expect("test source should parse");
        assert_eq!(visitor.sqlite_write_issues.len(), 1);
    }

    #[test]
    fn sqlite_write_guard_accepts_reviewed_direct_write_exception() {
        let source = r#"
            async fn persist(pool: &SqlitePool) {
                // sqlite-write-guard: allow-direct-pool-write
                sqlx::query("UPDATE settings SET value = 1")
                    .execute(pool)
                    .await
                    .unwrap();
            }
        "#;
        let visitor = scan_source(Path::new("src/admin_runtime.rs"), source)
            .expect("test source should parse");
        assert!(visitor.sqlite_write_issues.is_empty());
    }
}
