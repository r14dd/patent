//! Hackage source — searches `https://hackage.haskell.org/packages/search`.
//!
//! Hackage is the Haskell package archive. The search API returns only package
//! names, so we batch-fetch cabal files in parallel to extract each package's
//! synopsis and homepage.

use futures::future::join_all;
use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://hackage.haskell.org";
const MAX_RESULTS: usize = 20;

/// Searches the Hackage package archive (Haskell).
#[derive(Debug, Clone)]
pub struct Hackage {
    client: reqwest::Client,
    base_url: String,
}

impl Hackage {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResult {
    name: String,
}

fn parse_cabal_field<'a>(cabal: &'a str, field: &str) -> Option<&'a str> {
    for line in cabal.lines() {
        let lower = line.to_lowercase();
        let trimmed = lower.trim_start();
        if trimmed.starts_with(field) {
            if let Some(rest) = line.split_once(':') {
                let val = rest.1.trim();
                if !val.is_empty() {
                    return Some(val);
                }
            }
        }
    }
    None
}

#[async_trait::async_trait]
impl SourceAdapter for Hackage {
    fn id(&self) -> Source {
        Source::Hackage
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/packages/search", self.base_url);
        let q = query.keywords.join(" ");

        let results: Vec<SearchResult> = self
            .client
            .get(&url)
            .query(&[("terms", q.as_str())])
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let names: Vec<String> = results
            .into_iter()
            .take(MAX_RESULTS)
            .map(|r| r.name)
            .collect();

        let cabal_futures: Vec<_> = names
            .iter()
            .map(|name| {
                let cabal_url = format!("{}/package/{}/{}.cabal", self.base_url, name, name);
                let client = self.client.clone();
                async move { client.get(&cabal_url).send().await?.text().await }
            })
            .collect();
        let cabal_results = join_all(cabal_futures).await;

        let mut matches = Vec::new();
        for (name, cabal_result) in names.into_iter().zip(cabal_results) {
            let (synopsis, homepage) = match cabal_result {
                Ok(cabal) => (
                    parse_cabal_field(&cabal, "synopsis")
                        .unwrap_or("")
                        .to_string(),
                    parse_cabal_field(&cabal, "homepage").map(|h| h.to_string()),
                ),
                Err(_) => (String::new(), None),
            };
            let url = homepage
                .filter(|h| !h.is_empty())
                .unwrap_or_else(|| format!("{}/package/{}", self.base_url, name));
            matches.push(Match {
                name,
                source: Source::Hackage,
                url,
                description: synopsis,
                popularity: None,
                similarity: 0.0,
            });
        }

        Ok(matches)
    }
}
