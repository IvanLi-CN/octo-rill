use std::{
    collections::BTreeMap,
    collections::BTreeSet,
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use syn::{
    Attribute, Block, Expr, ExprCall, ExprMethodCall, File, FnArg, Item, ItemFn, ItemMod, Local,
    Meta, Pat, Type, TypeParamBound,
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
    test_only_depth: usize,
    sqlite_writer_closure_depth: usize,
    read_only_transaction_depth: usize,
    pool_aliases: BTreeSet<String>,
    coordinator_facade_functions: BTreeSet<String>,
    read_only_transaction_functions: BTreeSet<String>,
}

impl<'ast> Visit<'ast> for SourceVisitor {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.inspect_attribute(attribute);
        visit::visit_attribute(self, attribute);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let previous_test_only_depth = self.test_only_depth;
        let previous_pool_aliases = self.pool_aliases.clone();
        let previous_read_only_transaction_depth = self.read_only_transaction_depth;
        if has_cfg_test(&item.attrs) {
            self.test_only_depth += 1;
        }
        for input in &item.sig.inputs {
            if let FnArg::Typed(pattern) = input
                && type_mentions_sqlite_pool(&pattern.ty)
                && let Pat::Ident(pattern) = &*pattern.pat
            {
                self.pool_aliases.insert(pattern.ident.to_string());
            }
        }
        if self
            .read_only_transaction_functions
            .contains(&item.sig.ident.to_string())
        {
            self.read_only_transaction_depth += 1;
        }
        visit::visit_item_fn(self, item);
        self.test_only_depth = previous_test_only_depth;
        self.pool_aliases = previous_pool_aliases;
        self.read_only_transaction_depth = previous_read_only_transaction_depth;
    }

    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        let previous_test_only_depth = self.test_only_depth;
        if has_cfg_test(&item.attrs) {
            self.test_only_depth += 1;
        }
        visit::visit_item_mod(self, item);
        self.test_only_depth = previous_test_only_depth;
    }

    fn visit_block(&mut self, block: &'ast Block) {
        let previous_pool_aliases = self.pool_aliases.clone();
        visit::visit_block(self, block);
        self.pool_aliases = previous_pool_aliases;
    }

    fn visit_local(&mut self, local: &'ast Local) {
        let is_pool_alias = matches!(&local.pat, Pat::Type(pattern) if type_mentions_sqlite_pool(&pattern.ty))
            || local
                .init
                .as_ref()
                .is_some_and(|init| expression_mentions_pool(&init.expr, &self.pool_aliases));
        visit::visit_local(self, local);
        if let Some(pattern) = pattern_identifier(&local.pat) {
            if is_pool_alias {
                self.pool_aliases.insert(pattern.ident.to_string());
            } else {
                self.pool_aliases.remove(&pattern.ident.to_string());
            }
        }
    }

    fn visit_expr_method_call(&mut self, expression: &'ast ExprMethodCall) {
        let method = expression.method.to_string();
        let is_sqlite_writer_call = is_sqlite_writer_expression(&expression.receiver)
            && matches!(
                method.as_str(),
                "write" | "write_foreground" | "write_with_priority" | "try_write"
            );

        let is_explicit_read_only_transaction =
            self.read_only_transaction_depth > 0 && expression.method == "begin";
        if self.test_only_depth == 0
            && is_direct_pool_write(expression, &self.pool_aliases)
            && self.sqlite_writer_closure_depth == 0
            && !is_explicit_read_only_transaction
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
                    .is_some_and(|segment| {
                        self.coordinator_facade_functions
                            .contains(&segment.ident.to_string())
                    })
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
        predicates.iter().any(cfg_predicate_is_test_only)
    })
}

fn cfg_predicate_is_test_only(predicate: &Meta) -> bool {
    match predicate {
        Meta::Path(path) => path.is_ident("test"),
        Meta::List(list) if list.path.is_ident("all") => {
            let Ok(predicates) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            predicates.iter().any(cfg_predicate_is_test_only)
        }
        Meta::List(list) if list.path.is_ident("any") => {
            let Ok(predicates) = list.parse_args_with(
                syn::punctuated::Punctuated::<Meta, syn::Token![,]>::parse_terminated,
            ) else {
                return false;
            };
            !predicates.is_empty() && predicates.iter().all(cfg_predicate_is_test_only)
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

fn type_mentions_sqlite_pool(ty: &Type) -> bool {
    match ty {
        Type::Array(array) => type_mentions_sqlite_pool(&array.elem),
        Type::Group(group) => type_mentions_sqlite_pool(&group.elem),
        Type::Paren(paren) => type_mentions_sqlite_pool(&paren.elem),
        Type::Path(path) => path
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "SqlitePool"),
        Type::Reference(reference) => type_mentions_sqlite_pool(&reference.elem),
        Type::Slice(slice) => type_mentions_sqlite_pool(&slice.elem),
        Type::Tuple(tuple) => tuple.elems.iter().any(type_mentions_sqlite_pool),
        _ => false,
    }
}

fn pattern_identifier(pattern: &Pat) -> Option<&syn::PatIdent> {
    match pattern {
        Pat::Ident(pattern) => Some(pattern),
        Pat::Type(pattern) => pattern_identifier(&pattern.pat),
        _ => None,
    }
}

fn expression_mentions_pool(expression: &Expr, pool_aliases: &BTreeSet<String>) -> bool {
    match expression {
        Expr::Field(field) => {
            matches!(&field.member, syn::Member::Named(member) if member == "pool")
                || expression_mentions_pool(&field.base, pool_aliases)
        }
        Expr::Path(path) => path.path.segments.last().is_some_and(|segment| {
            segment.ident == "pool" || pool_aliases.contains(&segment.ident.to_string())
        }),
        Expr::MethodCall(call) => expression_mentions_pool(&call.receiver, pool_aliases),
        Expr::Reference(reference) => expression_mentions_pool(&reference.expr, pool_aliases),
        Expr::Paren(paren) => expression_mentions_pool(&paren.expr, pool_aliases),
        Expr::Group(group) => expression_mentions_pool(&group.expr, pool_aliases),
        Expr::Cast(cast) => expression_mentions_pool(&cast.expr, pool_aliases),
        Expr::Unary(unary) => expression_mentions_pool(&unary.expr, pool_aliases),
        Expr::Try(try_expression) => expression_mentions_pool(&try_expression.expr, pool_aliases),
        _ => false,
    }
}

fn is_direct_pool_write(expression: &ExprMethodCall, pool_aliases: &BTreeSet<String>) -> bool {
    if expression.method == "execute" {
        return expression
            .args
            .last()
            .is_some_and(|argument| expression_mentions_pool(argument, pool_aliases));
    }

    (expression.method == "begin" && expression_mentions_pool(&expression.receiver, pool_aliases))
        || (expression.method == "begin_with"
            && expression_mentions_pool(&expression.receiver, pool_aliases)
            && expression.args.first().is_some_and(|argument| {
                matches!(
                    argument,
                    Expr::Lit(literal)
                        if matches!(&literal.lit, syn::Lit::Str(value) if value.value().contains("BEGIN IMMEDIATE"))
                )
            }))
}

#[derive(Default)]
struct CoordinatorFacadeVisitor {
    uses_sqlite_writer: bool,
    forwards_callback: bool,
    callback_parameters: BTreeSet<String>,
}

struct CallbackReferenceVisitor<'a> {
    callback_parameters: &'a BTreeSet<String>,
    found: bool,
}

impl<'ast> Visit<'ast> for CallbackReferenceVisitor<'_> {
    fn visit_expr_path(&mut self, expression: &'ast syn::ExprPath) {
        if expression
            .path
            .get_ident()
            .is_some_and(|ident| self.callback_parameters.contains(&ident.to_string()))
        {
            self.found = true;
        }
        visit::visit_expr_path(self, expression);
    }
}

fn trait_path_is_callback(path: &syn::Path) -> bool {
    path.segments.last().is_some_and(|segment| {
        matches!(
            segment.ident.to_string().as_str(),
            "Fn" | "FnMut" | "FnOnce"
        )
    })
}

fn type_bound_is_callback(bound: &TypeParamBound) -> bool {
    matches!(bound, TypeParamBound::Trait(bound) if trait_path_is_callback(&bound.path))
}

fn callback_parameter_names(item: &ItemFn) -> BTreeSet<String> {
    let mut callback_type_parameters = BTreeSet::new();
    for parameter in &item.sig.generics.params {
        if let syn::GenericParam::Type(parameter) = parameter
            && parameter.bounds.iter().any(type_bound_is_callback)
        {
            callback_type_parameters.insert(parameter.ident.to_string());
        }
    }
    if let Some(where_clause) = &item.sig.generics.where_clause {
        for predicate in &where_clause.predicates {
            if let syn::WherePredicate::Type(predicate) = predicate
                && predicate.bounds.iter().any(type_bound_is_callback)
                && let Type::Path(path) = &predicate.bounded_ty
                && let Some(segment) = path.path.segments.last()
            {
                callback_type_parameters.insert(segment.ident.to_string());
            }
        }
    }

    item.sig
        .inputs
        .iter()
        .filter_map(|input| {
            let FnArg::Typed(pattern) = input else {
                return None;
            };
            let pattern_ident = pattern_identifier(&pattern.pat)?;
            let is_callback = match pattern.ty.as_ref() {
                Type::ImplTrait(ty) => ty.bounds.iter().any(type_bound_is_callback),
                Type::TraitObject(ty) => ty.bounds.iter().any(type_bound_is_callback),
                Type::Path(path) => path.path.segments.last().is_some_and(|segment| {
                    callback_type_parameters.contains(&segment.ident.to_string())
                }),
                _ => false,
            };
            is_callback.then(|| pattern_ident.ident.to_string())
        })
        .collect()
}

impl<'ast> Visit<'ast> for CoordinatorFacadeVisitor {
    fn visit_expr_method_call(&mut self, expression: &'ast ExprMethodCall) {
        if is_sqlite_writer_expression(&expression.receiver)
            && matches!(
                expression.method.to_string().as_str(),
                "write" | "write_foreground" | "write_with_priority" | "try_write"
            )
        {
            self.uses_sqlite_writer = true;
            for argument in &expression.args {
                let mut callback_reference = CallbackReferenceVisitor {
                    callback_parameters: &self.callback_parameters,
                    found: false,
                };
                callback_reference.visit_expr(argument);
                if callback_reference.found {
                    self.forwards_callback = true;
                }
            }
        }
        visit::visit_expr_method_call(self, expression);
    }
}

impl CoordinatorFacadeVisitor {
    fn for_item(item: &ItemFn) -> Self {
        Self {
            callback_parameters: callback_parameter_names(item),
            ..Self::default()
        }
    }
}

#[derive(Default)]
struct CoordinatorFacadeDeclarations {
    functions: BTreeSet<String>,
    read_only_transaction_functions: BTreeSet<String>,
    issues: Vec<String>,
    source_lines: Vec<String>,
}

impl<'ast> Visit<'ast> for CoordinatorFacadeDeclarations {
    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        let line_index = item.span().start().line.saturating_sub(1);
        let has_marker = self
            .source_lines
            .get(line_index.saturating_sub(1))
            .is_some_and(|line| line.contains("sqlite-write-guard: coordinator-facade"));
        let has_read_only_transaction_marker = self
            .source_lines
            .get(line_index.saturating_sub(1))
            .is_some_and(|line| line.contains("sqlite-write-guard: read-only-transaction"));
        if has_read_only_transaction_marker {
            self.read_only_transaction_functions
                .insert(item.sig.ident.to_string());
        }
        if has_marker {
            let mut verifier = CoordinatorFacadeVisitor::for_item(item);
            verifier.visit_item_fn(item);
            if verifier.uses_sqlite_writer && verifier.forwards_callback {
                self.functions.insert(item.sig.ident.to_string());
            } else {
                self.issues.push(format!(
                    "{}:{}: coordinator facade marker requires a SqliteWriteCoordinator call that forwards a callback parameter",
                    "source",
                    item.span().start().line
                ));
            }
        }
        visit::visit_item_fn(self, item);
    }
}

fn coordinator_facade_declarations(
    file: &File,
    source_lines: Vec<String>,
) -> CoordinatorFacadeDeclarations {
    let mut declarations = CoordinatorFacadeDeclarations {
        source_lines,
        ..CoordinatorFacadeDeclarations::default()
    };
    declarations.visit_file(file);
    declarations
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
    let source_lines = source.lines().map(str::to_owned).collect::<Vec<_>>();
    let facades = coordinator_facade_declarations(&file, source_lines.clone());
    let mut visitor = SourceVisitor {
        path: relative_path.clone(),
        coordinator_facade_functions: facades.functions,
        read_only_transaction_functions: facades.read_only_transaction_functions,
        ..SourceVisitor::default()
    };
    visitor.visit_file(&file);
    visitor.structural_issues.extend(facades.issues);
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
    fn sqlite_write_guard_rejects_nonstandard_pool_parameters() {
        let source = r#"
            async fn persist(db: &SqlitePool) {
                sqlx::query("UPDATE settings SET value = 1")
                    .execute(db)
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
            // sqlite-write-guard: coordinator-facade
            async fn run_subscription_prune_phase(state: &AppState, query: impl FnOnce()) {
                state.sqlite_writer.try_write("prune", query).await;
            }

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
    fn sqlite_write_guard_rejects_pool_aliases() {
        let source = r#"
            async fn persist(state: &AppState) {
                let db = &state.pool;
                sqlx::query("UPDATE settings SET value = 1")
                    .execute(db)
                    .await
                    .unwrap();
            }
        "#;
        let visitor = scan_source(Path::new("src/admin_runtime.rs"), source)
            .expect("test source should parse");
        assert_eq!(visitor.sqlite_write_issues.len(), 1);
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
    fn sqlite_write_guard_does_not_treat_cfg_any_test_as_test_only() {
        let source = r#"
            #[cfg(any(test, feature = "production"))]
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
    fn sqlite_write_guard_accepts_cfg_all_test_as_test_only() {
        let source = r#"
            #[cfg(all(test, feature = "fixture"))]
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
    fn sqlite_write_guard_rejects_unvalidated_coordinator_facade_callbacks() {
        let source = r#"
            // sqlite-write-guard: coordinator-facade
            async fn run_subscription_prune_phase(state: &AppState, query: impl FnOnce()) {
                state.sqlite_writer.try_write("prune", |_| async { Ok(()) }).await;
                query();
            }

            async fn persist(state: &AppState) {
                run_subscription_prune_phase(state, || async {
                    sqlx::query("DELETE FROM settings")
                        .execute(&state.pool)
                        .await
                        .unwrap();
                });
            }
        "#;
        let visitor =
            scan_source(Path::new("src/sync.rs"), source).expect("test source should parse");
        assert_eq!(visitor.sqlite_write_issues.len(), 1);
        assert!(!visitor.structural_issues.is_empty());
    }

    #[test]
    fn sqlite_write_guard_rejects_uncoordinated_pool_transactions() {
        let source = r#"
            async fn inspect(db: &SqlitePool) {
                let _tx = db.begin().await.unwrap();
            }
        "#;
        let visitor = scan_source(Path::new("src/translations.rs"), source)
            .expect("test source should parse");
        assert_eq!(visitor.sqlite_write_issues.len(), 1);
    }

    #[test]
    fn sqlite_write_guard_allows_marked_read_only_pool_transactions() {
        let source = r#"
            // sqlite-write-guard: read-only-transaction
            async fn inspect(db: &SqlitePool) {
                let _tx = db.begin().await.unwrap();
            }
        "#;
        let visitor = scan_source(Path::new("src/translations.rs"), source)
            .expect("test source should parse");
        assert!(visitor.sqlite_write_issues.is_empty());
    }
}
