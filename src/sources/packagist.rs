//! Packagist source — `GET https://packagist.org/search.json?q=`.
//!
//! Packagist is the main PHP/Composer package registry. Its search endpoint
//! returns clean JSON, so this mirrors the crates.io / npm adapters.

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

/// Default Packagist host. Overridable in tests via [`Packagist::with_base_url`].
const DEFAULT_BASE_URL: &str = "https://packagist.org";

/// Searches the Packagist (PHP/Composer) registry.
#[derive(Debug, Clone)]
pub struct Packagist {
    client: reqwest::Client,
    base_url: String,
}

impl Packagist {
    /// Construct against the live Packagist host.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests to point at a mock
    /// server). `base_url` should have no trailing slash.
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

/// Top-level shape of the Packagist search response.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<PackageHit>,
}

/// A single package in the `results` array. Only the fields we surface are
/// decoded.
#[derive(Debug, Deserialize)]
struct PackageHit {
    name: String,
    #[serde(default)]
    description: Option<String>,
    /// The Packagist package page; Packagist returns this as an absolute URL.
    #[serde(default)]
    url: Option<String>,
    /// Total install count; the primary popularity signal.
    #[serde(default)]
    downloads: Option<u64>,
    /// Stars ("favers"); used as a fallback when downloads are absent or zero.
    #[serde(default)]
    favers: Option<u64>,
}

#[async_trait::async_trait]
impl SourceAdapter for Packagist {
    fn id(&self) -> Source {
        Source::Packagist
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/search.json", self.base_url);

        // Packagist ANDs every term, so a realistic multi-keyword idea returns
        // nothing even where a matching package exists -- measured live, a
        // 7-term idea returns zero while its 2 longest terms return plenty.
        let mut matches = Vec::new();
        for q in super::narrowing_candidates(query, 2) {
            let response = self
                .client
                .get(&url)
                .query(&[("q", q.as_str()), ("per_page", "15")])
                .send()
                .await?
                .error_for_status()?;

            let body: SearchResponse = response.json().await?;

            matches = body
                .results
                .into_iter()
                .map(|p| {
                    // popularity = downloads, falling back to favers; never zero.
                    let popularity = p
                        .downloads
                        .filter(|&d| d > 0)
                        .or_else(|| p.favers.filter(|&f| f > 0));
                    Match {
                        url: p
                            .url
                            .unwrap_or_else(|| format!("{}/packages/{}", self.base_url, p.name)),
                        name: p.name,
                        source: Source::Packagist,
                        description: p.description.unwrap_or_default(),
                        popularity,
                        similarity: 0.0,
                        last_updated: None,
                    }
                })
                .collect();
            if !matches.is_empty() {
                break;
            }
        }
        Ok(matches)
    }
}
