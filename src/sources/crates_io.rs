//! crates.io source — `GET https://crates.io/api/v1/crates?q=`.

use serde::Deserialize;

use super::SourceAdapter;
use crate::freshness;
use crate::model::{Match, Query, Source};
use crate::Result;

/// Default crates.io host. Overridable in tests via [`CratesIo::with_base_url`].
const DEFAULT_BASE_URL: &str = "https://crates.io";

/// Searches the crates.io registry.
#[derive(Debug, Clone)]
pub struct CratesIo {
    client: reqwest::Client,
    base_url: String,
}

impl CratesIo {
    /// Construct against the live crates.io host.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests to point at a mock
    /// server). `base_url` should have no trailing slash.
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

/// Top-level shape of the crates.io search response.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    crates: Vec<CrateHit>,
}

/// A single crate in the `crates` array. Only the fields we surface are decoded.
#[derive(Debug, Deserialize)]
struct CrateHit {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
    /// Last publish of any version — RFC 3339 with microsecond precision.
    #[serde(default, deserialize_with = "crate::freshness::lenient")]
    updated_at: Option<String>,
}

#[async_trait::async_trait]
impl SourceAdapter for CratesIo {
    fn id(&self) -> Source {
        Source::CratesIo
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/api/v1/crates", self.base_url);

        // crates.io ANDs every term across name and description -- measured
        // live, a 7-term idea returns 3 unrelated crates and a 5-term one 121,
        // while 2 of its longest terms return tens of thousands.
        let mut matches = Vec::new();
        for q in super::narrowing_candidates(query, 2) {
            let response = self
                .client
                .get(&url)
                .query(&[("q", q.as_str()), ("per_page", "20")])
                .send()
                .await?
                .error_for_status()?;

            let body: SearchResponse = response.json().await?;

            matches = body
                .crates
                .into_iter()
                .map(|c| Match {
                    url: format!("{}/crates/{}", self.base_url, c.name),
                    name: c.name,
                    source: Source::CratesIo,
                    description: c.description.unwrap_or_default(),
                    popularity: c.downloads,
                    similarity: 0.0,
                    last_updated: c.updated_at.as_deref().and_then(freshness::from_rfc3339),
                })
                .collect();
            if !matches.is_empty() {
                break;
            }
        }
        Ok(matches)
    }
}
