//! RubyGems source — `GET https://rubygems.org/api/v1/search.json?query=`.

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://rubygems.org";

#[derive(Debug, Clone)]
pub struct RubyGems {
    client: reqwest::Client,
    base_url: String,
}

impl RubyGems {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct Gem {
    name: String,
    #[serde(default)]
    info: Option<String>,
    #[serde(default)]
    project_uri: Option<String>,
    #[serde(default)]
    downloads: Option<u64>,
}

#[async_trait::async_trait]
impl SourceAdapter for RubyGems {
    fn id(&self) -> Source {
        Source::RubyGems
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/api/v1/search.json", self.base_url);

        // RubyGems ANDs every term, so a realistic multi-keyword idea returns
        // nothing even where a matching gem exists -- measured live, a 7-term
        // idea returns zero while its 2 longest terms return plenty.
        let mut matches = Vec::new();
        for q in super::narrowing_candidates(query, 2) {
            let gems: Vec<Gem> = self
                .client
                .get(&url)
                .query(&[("query", q.as_str())])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            matches = gems
                .into_iter()
                .filter(|g| g.info.as_ref().is_some_and(|i| !i.is_empty()))
                .take(20)
                .map(|g| Match {
                    url: g
                        .project_uri
                        .unwrap_or_else(|| format!("https://rubygems.org/gems/{}", g.name)),
                    name: g.name,
                    source: Source::RubyGems,
                    description: g.info.unwrap_or_default(),
                    popularity: g.downloads,
                    similarity: 0.0,
                    last_updated: None,
                })
                .collect();
            if !matches.is_empty() {
                break;
            }
        }
        Ok(matches)
    }
}
