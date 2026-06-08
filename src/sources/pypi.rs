//! PyPI source.
//!
//! PyPI has no public search API (the XML-RPC endpoint was disabled), so this
//! scrapes the `https://pypi.org/search/?q=` results page with CSS selectors.
//! Brittle by nature — if the markup changes the parse yields nothing, which is
//! treated like any empty result (and the run is never blocked on it).

use scraper::{Html, Selector};

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::{Error, Result};

const DEFAULT_BASE_URL: &str = "https://pypi.org";
/// Searches PyPI (scrape-based; see module note).
#[derive(Debug, Clone)]
pub struct PyPI {
    client: reqwest::Client,
    base_url: String,
}

impl PyPI {
    /// Construct against the live PyPI site.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests).
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[async_trait::async_trait]
impl SourceAdapter for PyPI {
    fn id(&self) -> Source {
        Source::PyPI
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/search/", self.base_url);
        let q = query.keywords.join(" ");

        let html = self
            .client
            .get(&url)
            .query(&[("q", q.as_str())])
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        parse_search_html(&html, &self.base_url)
    }
}

/// Parse a PyPI search results page into matches. A package with no name is
/// skipped; a missing description becomes empty.
fn parse_search_html(html: &str, base_url: &str) -> Result<Vec<Match>> {
    let snippet = Selector::parse("a.package-snippet")
        .map_err(|e| Error::Parse(format!("bad selector: {e}")))?;
    let name = Selector::parse(".package-snippet__name")
        .map_err(|e| Error::Parse(format!("bad selector: {e}")))?;
    let description = Selector::parse(".package-snippet__description")
        .map_err(|e| Error::Parse(format!("bad selector: {e}")))?;

    let document = Html::parse_document(html);
    let mut matches = Vec::new();

    for element in document.select(&snippet) {
        let Some(name_text) = element
            .select(&name)
            .next()
            .map(|n| n.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };

        let description_text = element
            .select(&description)
            .next()
            .map(|d| d.text().collect::<String>().trim().to_string())
            .unwrap_or_default();

        let href = element.value().attr("href").unwrap_or("");
        let url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{base_url}{href}")
        };

        matches.push(Match {
            name: name_text,
            source: Source::PyPI,
            url,
            description: description_text,
            popularity: None,
            similarity: 0.0,
        });
    }

    Ok(matches)
}
