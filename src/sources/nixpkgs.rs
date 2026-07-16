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
                        "operator": "and",
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
            "{}/backend/latest-42-nixos-{}/_search",
            self.base_url, CHANNEL
        );

        let body = build_es_query(&q, MAX_RESULTS);

        let resp: EsResponse = self
            .client
            .post(&url)
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
                }
            })
            .collect();

        Ok(matches)
    }
}
