//! AUR (Arch User Repository) source.
//!
//! `GET https://aur.archlinux.org/rpc/?v=5&type=search&arg=` returns a clean
//! JSON document `{ resultcount, results: [{ Name, Description, URL, NumVotes,
//! Popularity, … }] }`. The result `URL` field is the upstream project link;
//! the canonical match URL is the AUR package page
//! (`/packages/{Name}`), and the popularity signal is `NumVotes`.

use serde::Deserialize;

use super::SourceAdapter;
use crate::freshness;
use crate::model::{Match, Query, Source};
use crate::Result;

/// Default AUR host. Overridable in tests via [`Aur::with_base_url`].
const DEFAULT_BASE_URL: &str = "https://aur.archlinux.org";

/// Searches the Arch User Repository.
#[derive(Debug, Clone)]
pub struct Aur {
    client: reqwest::Client,
    base_url: String,
}

impl Aur {
    /// Construct against the live AUR host.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests to point at a mock
    /// server). `base_url` should have no trailing slash.
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

/// Top-level shape of the AUR RPC search response.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    results: Vec<PackageHit>,
}

/// A single package in the `results` array. Only the fields we surface are
/// decoded; the rest of the (large) RPC payload is ignored.
#[derive(Debug, Deserialize)]
struct PackageHit {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Description", default)]
    description: Option<String>,
    #[serde(rename = "NumVotes", default)]
    num_votes: Option<u64>,
    /// Last change to the package's AUR entry, as epoch **seconds**.
    #[serde(
        rename = "LastModified",
        default,
        deserialize_with = "crate::freshness::lenient"
    )]
    last_modified: Option<i64>,
}

#[async_trait::async_trait]
impl SourceAdapter for Aur {
    fn id(&self) -> Source {
        Source::Aur
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/rpc/", self.base_url);
        let arg = query.keywords.join(" ");

        let body: SearchResponse = self
            .client
            .get(&url)
            .query(&[("v", "5"), ("type", "search"), ("arg", arg.as_str())])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(body
            .results
            .into_iter()
            .map(|p| Match {
                url: format!("{}/packages/{}", self.base_url, p.name),
                name: p.name,
                source: Source::Aur,
                description: p.description.unwrap_or_default(),
                // NumVotes is the AUR's "how established is this" signal. Zero
                // (or absent) votes carries no signal, so it maps to None.
                popularity: p.num_votes.filter(|&v| v > 0),
                similarity: 0.0,
                last_updated: p.last_modified.and_then(freshness::from_unix_secs),
            })
            .collect())
    }
}
