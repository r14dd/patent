//! npm source — `GET https://registry.npmjs.org/-/v1/search?text=`.

use serde::Deserialize;

use super::SourceAdapter;
use crate::freshness;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://registry.npmjs.org";

/// Searches the npm registry.
#[derive(Debug, Clone)]
pub struct Npm {
    client: reqwest::Client,
    base_url: String,
}

impl Npm {
    /// Construct against the live npm registry.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests).
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    objects: Vec<SearchObject>,
}

#[derive(Debug, Deserialize)]
struct SearchObject {
    package: Package,
    #[serde(default)]
    score: Option<Score>,
}

#[derive(Debug, Deserialize)]
struct Score {
    #[serde(default)]
    detail: ScoreDetail,
}

#[derive(Debug, Default, Deserialize)]
struct ScoreDetail {
    #[serde(default)]
    popularity: f32,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    #[serde(default)]
    description: Option<String>,
    /// Publish date of the version this hit refers to — RFC 3339, milliseconds.
    #[serde(default, deserialize_with = "crate::freshness::lenient")]
    date: Option<String>,
}

#[async_trait::async_trait]
impl SourceAdapter for Npm {
    fn id(&self) -> Source {
        Source::Npm
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/-/v1/search", self.base_url);
        let text = query.idea.clone();

        let body: SearchResponse = self
            .client
            .get(&url)
            .query(&[("text", text.as_str()), ("size", "20")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(body
            .objects
            .into_iter()
            .map(|o| {
                let pop = o
                    .score
                    .map(|s| (s.detail.popularity * 1_000_000.0) as u64)
                    .filter(|&p| p > 0);
                Match {
                    url: format!("https://www.npmjs.com/package/{}", o.package.name),
                    name: o.package.name,
                    source: Source::Npm,
                    description: o.package.description.unwrap_or_default(),
                    popularity: pop,
                    similarity: 0.0,
                    last_updated: o.package.date.as_deref().and_then(freshness::from_rfc3339),
                }
            })
            .collect())
    }
}
