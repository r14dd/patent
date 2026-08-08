//! JetBrains Marketplace source — `GET https://plugins.jetbrains.com/api/searchPlugins`
//! (free, no key).

use serde::Deserialize;

use super::SourceAdapter;
use crate::freshness;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://plugins.jetbrains.com";
const MAX_DESC_LEN: usize = 120;

#[derive(Debug, Clone)]
pub struct JetBrains {
    client: reqwest::Client,
    base_url: String,
}

impl JetBrains {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    plugins: Vec<Plugin>,
}

#[derive(Debug, Deserialize)]
struct Plugin {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    link: Option<String>,
    #[serde(default)]
    preview: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    #[serde(default, deserialize_with = "crate::freshness::lenient")]
    cdate: Option<i64>,
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
}

/// Picks the `n` longest keywords, preserving their original order (not
/// sorted by length) — see [`SourceAdapter::search`] below for why.
fn narrowed(keywords: &[String], n: usize) -> String {
    let mut idx: Vec<usize> = (0..keywords.len()).collect();
    idx.sort_by(|&a, &b| keywords[b].len().cmp(&keywords[a].len()));
    idx.truncate(n);
    idx.sort_unstable();
    idx.into_iter()
        .map(|i| keywords[i].as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

#[async_trait::async_trait]
impl SourceAdapter for JetBrains {
    fn id(&self) -> Source {
        Source::JetBrains
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/api/searchPlugins", self.base_url);

        // `searchPlugins` ANDs every content word in the query, so a
        // realistic multi-keyword idea (5+ terms) reliably returns zero hits
        // even when a matching plugin exists — measured live: a 5-7 term idea
        // returns `total: 0`, while the 3 longest terms of the same idea
        // routinely return results. Progressively narrow to fewer, longer
        // (more content-bearing) keywords until something comes back, skipping
        // any narrowing that repeats a query string already tried (e.g. with
        // ≤3 keywords, "3 longest" is identical to the full set).
        let mut candidates = Vec::new();
        for q in [
            query.keywords.join(" "),
            narrowed(&query.keywords, 3),
            narrowed(&query.keywords, 2),
        ] {
            if !q.is_empty() && !candidates.contains(&q) {
                candidates.push(q);
            }
        }
        if candidates.is_empty() {
            // No keywords at all: fall back to the raw idea rather than
            // sending an empty `search` param.
            candidates.push(query.idea.clone());
        }

        let mut plugins = Vec::new();
        for q in &candidates {
            let body: SearchResponse = self
                .client
                .get(&url)
                .query(&[("search", q.as_str()), ("max", "20")])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;
            if !body.plugins.is_empty() {
                plugins = body.plugins;
                break;
            }
        }

        Ok(plugins
            .into_iter()
            .filter(|p| p.name.as_ref().is_some_and(|n| !n.trim().is_empty()))
            .filter(|p| p.link.as_ref().is_some_and(|l| l.trim().starts_with('/')))
            .map(|p| {
                let name = p.name.unwrap_or_default();
                let link = p.link.unwrap_or_default();
                let url = format!("{}{}", self.base_url, link.trim());
                let desc = p
                    .preview
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(name.as_str());
                let desc = truncate(desc, MAX_DESC_LEN);
                Match {
                    name,
                    source: Source::JetBrains,
                    url,
                    description: desc,
                    popularity: p.downloads,
                    similarity: 0.0,
                    last_updated: p.cdate.and_then(freshness::from_unix_millis),
                }
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::narrowed;

    fn kw(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    #[test]
    fn narrowed_picks_longest_but_preserves_original_order() {
        // Lengths: ide=3, code=4, spell=5, syntax=6, mistakes=8. The 3
        // longest are mistakes/syntax/spell, but the result must come back
        // in their original (index) order, not length order.
        let keywords = kw(&["ide", "code", "spell", "syntax", "mistakes"]);
        assert_eq!(narrowed(&keywords, 3), "spell syntax mistakes");
        assert_eq!(narrowed(&keywords, 2), "syntax mistakes");
    }

    #[test]
    fn narrowed_is_a_no_op_when_n_covers_all_keywords() {
        let keywords = kw(&["async", "runtime"]);
        assert_eq!(narrowed(&keywords, 3), "async runtime");
        assert_eq!(narrowed(&keywords, 2), "async runtime");
    }

    #[test]
    fn narrowed_of_empty_keywords_is_empty() {
        assert_eq!(narrowed(&[], 3), "");
    }
}
