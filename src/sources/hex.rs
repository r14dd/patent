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

/// How many of the longest keywords get their own single-term search.
const TERMS: usize = 3;

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

        // Hex's search grammar only honours a single bare term. Measured live:
        // a space-joined multi-word query returns the unfiltered catalogue
        // (100 packages, alphabetical, identical for every idea), and with
        // `description:` prefixes only the last term filters. Narrowing cannot
        // detect that -- the flood is never empty -- so each of the longest
        // keywords is searched on its own and the results are merged.
        let mut terms: Vec<&str> = query.keywords.iter().map(String::as_str).collect();
        terms.sort_by_key(|t| std::cmp::Reverse(t.len()));
        terms.truncate(TERMS);
        if terms.is_empty() {
            terms.push(query.idea.as_str());
        }

        let mut matches: Vec<Match> = Vec::new();
        for term in terms {
            let body: Vec<Package> = self
                .client
                .get(&url)
                .query(&[("search", term), ("page", "1")])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            for p in body {
                if matches.iter().any(|m| m.name == p.name) {
                    continue;
                }
                // Prefer the registry-provided web link; fall back to the
                // canonical package URL (honoring base_url so tests stay local).
                let url = p
                    .html_url
                    .filter(|u| !u.trim().is_empty())
                    .unwrap_or_else(|| format!("{}/packages/{}", self.base_url, p.name));
                matches.push(Match {
                    url,
                    name: p.name,
                    source: Source::Hex,
                    description: p.meta.description.unwrap_or_default(),
                    popularity: p.downloads.all.filter(|&n| n > 0),
                    similarity: 0.0,
                    last_updated: p.updated_at.as_deref().and_then(freshness::from_rfc3339),
                });
            }
        }
        Ok(matches)
    }
}
