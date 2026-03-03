pub const SOURCE_APP_EFFECTIVE_VERSION: &str = "APP_EFFECTIVE_VERSION";
pub const SOURCE_CARGO_PKG_VERSION: &str = "CARGO_PKG_VERSION";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionInfo {
    pub version: String,
    pub source: &'static str,
}

#[must_use]
pub fn resolve_effective_version() -> VersionInfo {
    resolve_effective_version_from(
        option_env!("APP_EFFECTIVE_VERSION"),
        env!("CARGO_PKG_VERSION"),
    )
}

fn resolve_effective_version_from(
    app_effective_version: Option<&str>,
    cargo_pkg_version: &str,
) -> VersionInfo {
    let effective = app_effective_version
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match effective {
        Some(version) => VersionInfo {
            version: version.to_owned(),
            source: SOURCE_APP_EFFECTIVE_VERSION,
        },
        None => VersionInfo {
            version: cargo_pkg_version.to_owned(),
            source: SOURCE_CARGO_PKG_VERSION,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::{
        SOURCE_APP_EFFECTIVE_VERSION, SOURCE_CARGO_PKG_VERSION, resolve_effective_version_from,
    };

    #[test]
    fn prefers_non_empty_app_effective_version() {
        let info = resolve_effective_version_from(Some("1.2.3"), "0.1.0");
        assert_eq!(info.version, "1.2.3");
        assert_eq!(info.source, SOURCE_APP_EFFECTIVE_VERSION);
    }

    #[test]
    fn trims_app_effective_version_before_use() {
        let info = resolve_effective_version_from(Some("  2.3.4  "), "0.1.0");
        assert_eq!(info.version, "2.3.4");
        assert_eq!(info.source, SOURCE_APP_EFFECTIVE_VERSION);
    }

    #[test]
    fn falls_back_when_app_effective_version_is_blank() {
        let info = resolve_effective_version_from(Some("   "), "0.1.0");
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.source, SOURCE_CARGO_PKG_VERSION);
    }

    #[test]
    fn falls_back_when_app_effective_version_is_missing() {
        let info = resolve_effective_version_from(None, "0.1.0");
        assert_eq!(info.version, "0.1.0");
        assert_eq!(info.source, SOURCE_CARGO_PKG_VERSION);
    }
}
