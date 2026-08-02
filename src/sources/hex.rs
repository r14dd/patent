//! Hex source — `GET https://hex.pm/api/packages?search=`.
//!
//! Hex is the package registry for the Erlang/Elixir ecosystem (Mix, Rebar3,
//! Phoenix). The search endpoint returns a JSON array of packages; each carries
//! a `meta.description`, an `html_url`, and a `downloads` block whose `all`
//! field is the lifetime download count we surface as popularity.

use serde::Deserialize;

use super::SourceAdapter;
use crate::freshness;
use crate::model::{Match, Query, Source};
use crate::Result;

/// Default Hex host. Overridable in tests via [`Hex::with_base_url`].
const DEFAULT_BASE_URL: &str = "https://hex.pm";

/// Searches the Hex registry (Erlang/Elixir).
#[derive(Debug, Clone)]
pub struct Hex {
    client: reqwest::Client,
    base_url: String,
}

impl Hex {
    /// Construct against the live Hex host.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests to point at a mock
    /// server). `base_url` should have no trailing slash.
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

/// A single package in the Hex search response array. Only the fields we
/// surface are decoded.
#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    #[serde(default)]
    html_url: Option<String>,
    #[serde(default)]
    meta: Meta,
    #[serde(default)]
    downloads: Downloads,
    /// Last release of the package — RFC 3339 with microsecond precision.
    #[serde(default, deserialize_with = "crate::freshness::lenient")]
    updated_at: Option<String>,
}

/// The `meta` block; only the human-readable description is used.
#[derive(Debug, Default, Deserialize)]
struct Meta {
    #[serde(default)]
    description: Option<String>,
}

/// The `downloads` block; `all` is the lifetime count used as popularity.
#[derive(Debug, Default, Deserialize)]
struct Downloads {
    #[serde(default)]
    all: Option<u64>,
}

#[async_trait::async_trait]
impl SourceAdapter for Hex {
    fn id(&self) -> Source {
        Source::Hex
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/api/packages", self.base_url);
        let q = query.keywords.join(" ");

        let body: Vec<Package> = self
            .client
            .get(&url)
            .query(&[("search", q.as_str()), ("page", "1")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(body
            .into_iter()
            .map(|p| {
                // Prefer the registry-provided web link; fall back to the
                // canonical package URL (honoring base_url so tests stay local).
                let url = p
                    .html_url
                    .filter(|u| !u.trim().is_empty())
                    .unwrap_or_else(|| format!("{}/packages/{}", self.base_url, p.name));
                Match {
                    name: p.name,
                    source: Source::Hex,
                    url,
                    description: p.meta.description.unwrap_or_default(),
                    // Treat a zero/absent lifetime download count as no signal.
                    popularity: p.downloads.all.filter(|&d| d > 0),
                    similarity: 0.0,
                    last_updated: p.updated_at.as_deref().and_then(freshness::from_rfc3339),
                }
            })
            .collect())
    }
}
