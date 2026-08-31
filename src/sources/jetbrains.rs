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
        // routinely return results.
        let candidates = super::narrowing_candidates(query, 2);

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
