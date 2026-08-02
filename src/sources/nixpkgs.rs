//! Nixpkgs source — searches `search.nixos.org` for Nix packages.
//!
//! Uses the Elasticsearch backend that powers the NixOS package search website.
//! Queries the `nixos-unstable` channel by default.

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://search.nixos.org";
const CHANNEL: &str = "unstable";
const MAX_RESULTS: usize = 20;

/// The backend is not open to anonymous clients — it rejects unauthenticated
/// requests with `401`. These are the read-only credentials that
/// `search.nixos.org` itself ships to every browser that loads the page (they
/// are checked into the public `NixOS/nixos-search` repository); they are not a
/// secret and grant nothing beyond the same public query access.
const ES_USER: &str = "aWVSALXpZv";
const ES_PASSWORD: &str = "X8gPHnzL52wFEekuxsfQ9cSh";

/// Index names embed a schema generation that upstream bumps whenever the
/// document mapping changes; the old index is then deleted and requests for it
/// 404. It therefore has to be updated periodically — `live_nixpkgs` in
/// `tests/live.rs` is what catches the rot, since a mocked test cannot.
const INDEX_GENERATION: u32 = 50;

/// Searches the NixOS package index.
#[derive(Debug, Clone)]
pub struct Nixpkgs {
    client: reqwest::Client,
    base_url: String,
}

impl Nixpkgs {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct EsResponse {
    hits: EsHits,
}

#[derive(Debug, Deserialize)]
struct EsHits {
    hits: Vec<EsHit>,
}

#[derive(Debug, Deserialize)]
struct EsHit {
    _source: EsPackage,
}

#[derive(Debug, Deserialize)]
struct EsPackage {
    package_attr_name: String,
    package_pname: Option<String>,
    package_description: Option<String>,
    #[serde(default)]
    package_homepage: Vec<String>,
    package_pversion: Option<String>,
}

/// The query DSL below is lifted from the `search.nixos.org` frontend, which
/// feeds it a one- or two-word search box. This tool feeds it a whole extracted
/// keyword list instead, so the frontend's `"operator": "and"` — every term must
/// match one package — made realistic multi-word ideas return nothing at all.
/// It is `"or"` here deliberately: casting wide and letting `rank.rs` sink the
/// loose hits is the same trade every other source makes (Hex returns 100 rows,
/// AUR and GitHub ~50). A *reached* source that is always empty is worse than a
/// failing one — it feeds a falsely-clean verdict instead of being surfaced as
/// "not reached".
fn build_es_query(terms: &str, size: usize) -> serde_json::Value {
    serde_json::json!({
        "from": 0,
        "size": size,
        "sort": [{"_score": "desc", "package_attr_name": "desc"}],
        "query": {
            "dis_max": {
                "tie_breaker": 0.27,
                "queries": [{
                    "multi_match": {
                        "type": "cross_fields",
                        "query": terms,
                        "analyzer": "whitespace",
                        "auto_generate_synonyms_phrase_query": false,
                        "operator": "or",
                        "fields": [
                            "package_attr_name^9",
                            "package_pname^6",
                            "package_description^1.3",
                            "package_longDescription^1"
                        ]
                    }
                }]
            }
        }
    })
}

#[async_trait::async_trait]
impl SourceAdapter for Nixpkgs {
    fn id(&self) -> Source {
        Source::Nixpkgs
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let q = query.keywords.join(" ");
        let url = format!(
            "{}/backend/latest-{}-nixos-{}/_search",
            self.base_url, INDEX_GENERATION, CHANNEL
        );

        let body = build_es_query(&q, MAX_RESULTS);

        let resp: EsResponse = self
            .client
            .post(&url)
            .basic_auth(ES_USER, Some(ES_PASSWORD))
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let matches = resp
            .hits
            .hits
            .into_iter()
            .map(|hit| {
                let pkg = hit._source;
                let name = pkg.package_pname.unwrap_or(pkg.package_attr_name.clone());
                let desc = pkg.package_description.unwrap_or_default();
                let url = pkg
                    .package_homepage
                    .into_iter()
                    .next()
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| {
                        format!(
                            "https://search.nixos.org/packages?channel={}&show={}&query={}",
                            CHANNEL, pkg.package_attr_name, name
                        )
                    });
                let version_suffix = pkg
                    .package_pversion
                    .filter(|v| !v.is_empty())
                    .map(|v| format!(" (v{v})"))
                    .unwrap_or_default();

                Match {
                    name: format!("{name}{version_suffix}"),
                    source: Source::Nixpkgs,
                    url,
                    description: desc,
                    popularity: None,
                    similarity: 0.0,
                    last_updated: None,
                }
            })
            .collect();

        Ok(matches)
    }
}
