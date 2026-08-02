//! VS Code Marketplace source — POST to the gallery extension query API.

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://marketplace.visualstudio.com";

#[derive(Debug, Clone)]
pub struct VsCodeMarketplace {
    client: reqwest::Client,
    base_url: String,
}

impl VsCodeMarketplace {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct GalleryResponse {
    results: Vec<GalleryResult>,
}

#[derive(Debug, Deserialize)]
struct GalleryResult {
    extensions: Vec<Extension>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Extension {
    publisher: Publisher,
    extension_name: String,
    #[serde(default)]
    display_name: Option<String>,
    #[serde(default)]
    short_description: Option<String>,
    #[serde(default)]
    statistics: Vec<Statistic>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Publisher {
    publisher_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Statistic {
    statistic_name: String,
    value: f64,
}

#[async_trait::async_trait]
impl SourceAdapter for VsCodeMarketplace {
    fn id(&self) -> Source {
        Source::VsCodeMarketplace
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/_apis/public/gallery/extensionquery", self.base_url);
        let q = query.keywords.join(" ");

        let body = serde_json::json!({
            "filters": [{
                "criteria": [
                    { "filterType": 8, "value": "Microsoft.VisualStudio.Code" },
                    { "filterType": 10, "value": q },
                ],
                "pageSize": 20,
                "pageNumber": 1,
            }],
            "flags": 914
        });

        let resp: GalleryResponse = self
            .client
            .post(&url)
            .header("Accept", "application/json;api-version=7.1-preview.1")
            .json(&body)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        let extensions = match resp.results.into_iter().next() {
            Some(r) => r.extensions,
            None => return Ok(vec![]),
        };

        Ok(extensions
            .into_iter()
            .filter(|e| e.short_description.as_ref().is_some_and(|d| !d.is_empty()))
            .map(|e| {
                let installs = e
                    .statistics
                    .iter()
                    .find(|s| s.statistic_name == "install")
                    .map(|s| s.value as u64);
                let display = e.display_name.unwrap_or_else(|| e.extension_name.clone());
                let url = format!(
                    "https://marketplace.visualstudio.com/items?itemName={}.{}",
                    e.publisher.publisher_name, e.extension_name,
                );
                Match {
                    name: display,
                    source: Source::VsCodeMarketplace,
                    url,
                    description: e.short_description.unwrap_or_default(),
                    popularity: installs,
                    similarity: 0.0,
                    last_updated: None,
                }
            })
            .collect())
    }
}
