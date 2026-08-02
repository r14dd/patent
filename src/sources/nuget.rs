//! NuGet source — `GET https://azuresearch-usnc.nuget.org/query?q=`.
//!
//! Covers C#, F#, VB.NET, and the broader .NET ecosystem.

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_SEARCH_URL: &str = "https://azuresearch-usnc.nuget.org";

#[derive(Debug, Clone)]
pub struct NuGet {
    client: reqwest::Client,
    search_url: String,
}

impl NuGet {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_search_url(client, DEFAULT_SEARCH_URL.to_string())
    }

    pub fn with_search_url(client: reqwest::Client, search_url: String) -> Self {
        Self { client, search_url }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    data: Vec<Package>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default, rename = "totalDownloads")]
    total_downloads: Option<u64>,
}

#[async_trait::async_trait]
impl SourceAdapter for NuGet {
    fn id(&self) -> Source {
        Source::NuGet
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/query", self.search_url);
        let q = query.keywords.join(" ");

        let body: SearchResponse = self
            .client
            .get(&url)
            .query(&[("q", q.as_str()), ("take", "20")])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(body
            .data
            .into_iter()
            .filter(|p| p.description.as_ref().is_some_and(|d| !d.is_empty()))
            .map(|p| Match {
                url: format!("https://www.nuget.org/packages/{}", p.id),
                name: p.id,
                source: Source::NuGet,
                description: p.description.unwrap_or_default(),
                popularity: p.total_downloads,
                similarity: 0.0,
                last_updated: None,
            })
            .collect())
    }
}
