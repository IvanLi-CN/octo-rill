use std::collections::{HashMap, HashSet};

use sqlx::Sqlite;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ReleaseLocator {
    pub owner: String,
    pub repo: String,
    pub tag: String,
}

impl ReleaseLocator {
    pub fn full_name(&self) -> String {
        format!("{}/{}", self.owner, self.repo)
    }

    pub fn matches(&self, other: &Self) -> bool {
        self.owner.eq_ignore_ascii_case(&other.owner)
            && self.repo.eq_ignore_ascii_case(&other.repo)
            && self.tag == other.tag
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum InternalReleaseRef {
    ReleaseId(i64),
    Locator(ReleaseLocator),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedReleaseRefs {
    pub ids: Vec<i64>,
    pub unresolved: Vec<InternalReleaseRef>,
}

#[derive(Debug, sqlx::FromRow)]
struct ReleaseTagLookupRow {
    release_id: i64,
    tag_name: String,
    html_url: String,
}

fn decode_path_segment(raw: &str) -> Option<String> {
    urlencoding::decode(raw)
        .ok()
        .map(|value| value.into_owned())
}

fn parse_release_locator_from_url(url: &Url) -> Option<ReleaseLocator> {
    let segments = url.path_segments()?.collect::<Vec<_>>();
    if segments.len() != 5 || segments[2] != "releases" || segments[3] != "tag" {
        return None;
    }

    let owner = decode_path_segment(segments[0])?;
    let repo = decode_path_segment(segments[1])?;
    let tag = decode_path_segment(segments[4])?;
    if owner.trim().is_empty() || repo.trim().is_empty() || tag.is_empty() {
        return None;
    }

    Some(ReleaseLocator { owner, repo, tag })
}

pub fn parse_repo_full_name_from_release_url(html_url: &str) -> Option<String> {
    parse_release_locator_from_github_release_url(html_url).map(|locator| locator.full_name())
}

pub fn parse_release_locator_from_github_release_url(html_url: &str) -> Option<ReleaseLocator> {
    let parsed = Url::parse(html_url).ok()?;
    let host = parsed.host_str()?;
    if host != "github.com" && host != "www.github.com" {
        return None;
    }
    parse_release_locator_from_url(&parsed)
}

pub fn locator_matches_github_release_url(locator: &ReleaseLocator, html_url: &str) -> bool {
    parse_release_locator_from_github_release_url(html_url)
        .is_some_and(|candidate| locator.matches(&candidate))
}

pub fn build_internal_release_href(locator: &ReleaseLocator, from: Option<&str>) -> String {
    let mut href = format!(
        "/{}/{}/releases/tag/{}",
        urlencoding::encode(&locator.owner),
        urlencoding::encode(&locator.repo),
        urlencoding::encode(&locator.tag),
    );
    if let Some(from) = from.map(str::trim).filter(|value| !value.is_empty()) {
        href.push_str("?from=");
        href.push_str(urlencoding::encode(from).as_ref());
    }
    href
}

pub fn build_internal_brief_release_href(locator: &ReleaseLocator) -> String {
    build_internal_release_href(locator, Some("briefs"))
}

pub fn build_internal_brief_release_href_from_html_url(html_url: &str) -> Option<String> {
    parse_release_locator_from_github_release_url(html_url)
        .map(|locator| build_internal_brief_release_href(&locator))
}

pub fn parse_internal_release_ref(target: &str) -> Option<InternalReleaseRef> {
    let base = Url::parse("https://octorill.local/").expect("valid local base url");
    let joined = base.join(target.trim()).ok()?;

    if joined.host_str() != Some("octorill.local") {
        return None;
    }

    let tab = joined
        .query_pairs()
        .find_map(|(key, value)| (key == "tab").then_some(value.into_owned()));
    let release = joined
        .query_pairs()
        .find_map(|(key, value)| (key == "release").then_some(value.into_owned()));
    if let Some(raw_release) = release
        && tab.as_deref().is_none_or(|value| value == "briefs")
        && raw_release.chars().all(|ch| ch.is_ascii_digit())
        && let Ok(release_id) = raw_release.parse::<i64>()
    {
        return Some(InternalReleaseRef::ReleaseId(release_id));
    }

    parse_release_locator_from_url(&joined).map(InternalReleaseRef::Locator)
}

pub async fn resolve_release_refs<'e, E>(
    executor: E,
    refs: &[InternalReleaseRef],
) -> Result<ResolvedReleaseRefs, sqlx::Error>
where
    E: sqlx::Executor<'e, Database = Sqlite>,
{
    let mut ids = Vec::new();
    let mut unresolved = Vec::new();
    let mut seen_ids = HashSet::<i64>::new();
    let mut seen_refs = HashSet::<InternalReleaseRef>::new();
    let mut locators = Vec::<ReleaseLocator>::new();

    for reference in refs {
        if !seen_refs.insert(reference.clone()) {
            continue;
        }
        match reference {
            InternalReleaseRef::ReleaseId(release_id) => {
                if seen_ids.insert(*release_id) {
                    ids.push(*release_id);
                }
            }
            InternalReleaseRef::Locator(locator) => locators.push(locator.clone()),
        }
    }

    if locators.is_empty() {
        return Ok(ResolvedReleaseRefs { ids, unresolved });
    }

    let mut tags = Vec::<String>::new();
    let mut seen_tags = HashSet::<String>::new();
    for locator in &locators {
        if seen_tags.insert(locator.tag.clone()) {
            tags.push(locator.tag.clone());
        }
    }

    let placeholders = vec!["?"; tags.len()].join(", ");
    let sql = format!(
        r#"
        SELECT release_id, tag_name, html_url
        FROM repo_releases
        WHERE tag_name IN ({placeholders})
        ORDER BY published_at DESC, created_at DESC, release_id DESC
        "#
    );
    let mut query = sqlx::query_as::<_, ReleaseTagLookupRow>(&sql);
    for tag in &tags {
        query = query.bind(tag);
    }

    let rows = query.fetch_all(executor).await?;
    let mut rows_by_tag = HashMap::<String, Vec<ReleaseTagLookupRow>>::new();
    for row in rows {
        rows_by_tag
            .entry(row.tag_name.clone())
            .or_default()
            .push(row);
    }

    for locator in locators {
        let Some(matched_id) = rows_by_tag.get(&locator.tag).and_then(|rows| {
            rows.iter()
                .find(|row| locator_matches_github_release_url(&locator, &row.html_url))
                .map(|row| row.release_id)
        }) else {
            unresolved.push(InternalReleaseRef::Locator(locator));
            continue;
        };

        if seen_ids.insert(matched_id) {
            ids.push(matched_id);
        }
    }

    Ok(ResolvedReleaseRefs { ids, unresolved })
}

#[cfg(test)]
mod tests {
    use super::{
        InternalReleaseRef, ReleaseLocator, build_internal_brief_release_href,
        locator_matches_github_release_url, parse_internal_release_ref,
        parse_release_locator_from_github_release_url,
    };

    #[test]
    fn parse_release_locator_from_github_release_url_decodes_tag_path_segment() {
        let locator = parse_release_locator_from_github_release_url(
            "https://github.com/acme/rocket/releases/tag/release%2F2026.04",
        )
        .expect("locator");
        assert_eq!(locator.owner, "acme");
        assert_eq!(locator.repo, "rocket");
        assert_eq!(locator.tag, "release/2026.04");
    }

    #[test]
    fn parse_internal_release_ref_accepts_legacy_query_links() {
        assert_eq!(
            parse_internal_release_ref("/?release=123&tab=briefs"),
            Some(InternalReleaseRef::ReleaseId(123))
        );
        assert_eq!(
            parse_internal_release_ref("/?release=123"),
            Some(InternalReleaseRef::ReleaseId(123))
        );
    }

    #[test]
    fn parse_internal_release_ref_accepts_canonical_path_links() {
        assert_eq!(
            parse_internal_release_ref("/acme/rocket/releases/tag/release%2F2026.04?from=briefs"),
            Some(InternalReleaseRef::Locator(ReleaseLocator {
                owner: "acme".to_owned(),
                repo: "rocket".to_owned(),
                tag: "release/2026.04".to_owned(),
            }))
        );
    }

    #[test]
    fn build_internal_brief_release_href_uses_canonical_path() {
        let href = build_internal_brief_release_href(&ReleaseLocator {
            owner: "acme".to_owned(),
            repo: "rocket".to_owned(),
            tag: "release/2026.04".to_owned(),
        });
        assert_eq!(
            href,
            "/acme/rocket/releases/tag/release%2F2026.04?from=briefs"
        );
    }

    #[test]
    fn locator_matches_release_url_ignores_repo_case_but_keeps_tag_exact() {
        let locator = ReleaseLocator {
            owner: "Acme".to_owned(),
            repo: "Rocket".to_owned(),
            tag: "release/2026.04".to_owned(),
        };
        assert!(locator_matches_github_release_url(
            &locator,
            "https://github.com/acme/rocket/releases/tag/release%2F2026.04"
        ));
        assert!(!locator_matches_github_release_url(
            &locator,
            "https://github.com/acme/rocket/releases/tag/release%2F2026.05"
        ));
    }
}
