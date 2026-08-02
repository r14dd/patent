//! Docker Hub source — `GET https://hub.docker.com/v2/search/repositories/`.

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://hub.docker.com";

#[derive(Debug, Clone)]
pub struct DockerHub {
    client: reqwest::Client,
    base_url: String,
}

impl DockerHub {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    results: Vec<Repo>,
}

#[derive(Debug, Deserialize)]
struct Repo {
    repo_name: String,
    #[serde(default)]
    short_description: Option<String>,
    #[serde(default)]
    star_count: Option<u64>,
}

#[async_trait::async_trait]
impl SourceAdapter for DockerHub {
    fn id(&self) -> Source {
        Source::DockerHub
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/v2/search/repositories/", self.base_url);
        let q = query.keywords.join(" ");

        let body: SearchResponse = self
            .client
            .get(&url)
            .query(&[("query", q.as_str()), ("page_size", "20")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(body
            .results
            .into_iter()
            .filter(|r| r.short_description.as_ref().is_some_and(|d| !d.is_empty()))
            .map(|r| Match {
                url: format!("https://hub.docker.com/r/{}", r.repo_name),
                name: r.repo_name,
                source: Source::DockerHub,
                description: r.short_description.unwrap_or_default(),
                popularity: r.star_count,
                similarity: 0.0,
                last_updated: None,
            })
            .collect())
    }
}
