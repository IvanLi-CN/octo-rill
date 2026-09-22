use std::{
    collections::BTreeMap,
    env, fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use syn::{
    Attribute, File, Item, Meta,
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
}

impl<'ast> Visit<'ast> for SourceVisitor {
    fn visit_attribute(&mut self, attribute: &'ast Attribute) {
        self.inspect_attribute(attribute);
        visit::visit_attribute(self, attribute);
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
        ..SourceVisitor::default()
    };
    visitor.visit_file(&file);
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
}
