//! Maven Central source — `GET https://search.maven.org/solrsearch/select`.
//!
//! The search API returns artifact coordinates but no descriptions, so the
//! artifact name is the main signal for ranking.
//!
//! **Popularity signal:** the Solr payload exposes `versionCount` (how many
//! releases the artifact has) but no download, star, or ranking field that
//! is comparable to what the other sources populate. Storing `versionCount`
//! as the `Match::popularity` would mis-label it in the TUI as a download
//! count, so we leave it as `None` and surface the raw field in the
//! description when callers need it.

use serde::Deserialize;

use super::SourceAdapter;
use crate::freshness;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://search.maven.org";

#[derive(Debug, Clone)]
pub struct Maven {
    client: reqwest::Client,
    base_url: String,
}

impl Maven {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct SolrResponse {
    response: SolrDocs,
}

#[derive(Debug, Deserialize)]
struct SolrDocs {
    docs: Vec<SolrDoc>,
}

#[derive(Debug, Deserialize)]
struct SolrDoc {
    #[serde(default)]
    g: String,
    #[serde(default)]
    a: String,
    /// Publish time of `latestVersion`, as epoch **milliseconds**.
    #[serde(default, deserialize_with = "crate::freshness::lenient")]
    timestamp: Option<i64>,
}

#[async_trait::async_trait]
impl SourceAdapter for Maven {
    fn id(&self) -> Source {
        Source::Maven
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/solrsearch/select", self.base_url);

        // Central's Solr index ANDs every term against artifact coordinates
        // only, which is far stricter than the other registries: measured live,
        // "kill process port" returns zero and even 2 terms return zero, while
        // "process" alone returns 845. Maven is the one source that has to
        // narrow all the way down to a single keyword to come back with
        // anything; the noise that lets in is dropped by similarity ranking.
        let mut matches = Vec::new();
        for q in super::narrowing_candidates(query, 1) {
            let body: SolrResponse = self
                .client
                .get(&url)
                .query(&[("q", q.as_str()), ("rows", "20"), ("wt", "json")])
                .send()
                .await?
                .error_for_status()?
                .json()
                .await?;

            matches = body
                .response
                .docs
                .into_iter()
                .map(|d| {
                    let name = format!("{}:{}", d.g, d.a);
                    let url = format!("https://central.sonatype.com/artifact/{}/{}", d.g, d.a);
                    Match {
                        description: d.a.clone(),
                        name,
                        source: Source::Maven,
                        url,
                        popularity: None,
                        similarity: 0.0,
                        last_updated: d.timestamp.and_then(freshness::from_unix_millis),
                    }
                })
                .collect();
            if !matches.is_empty() {
                break;
            }
        }
        Ok(matches)
    }
}
