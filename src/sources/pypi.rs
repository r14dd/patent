//! PyPI source.
//!
//! PyPI has no public search API (the XML-RPC endpoint was disabled), so this
//! scrapes the `https://pypi.org/search/?q=` results page with CSS selectors.
//! That page is now served behind a bot challenge to non-browser clients, so in
//! practice the scrape is walled: either a hard `403`, or a `200` whose body is a
//! JS challenge stub that matches no snippet. Both surface as [`Error::Unavailable`]
//! with accurate wording — never a silent empty result, and never the misleading
//! "markup may have changed" — so the fan-out reports PyPI as *not reached* (and
//! skips the retry it can't win) rather than mistaking the wall for "nothing out
//! there". A genuinely empty (small) results page is still a legitimate empty.

use scraper::{Html, Selector};

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::{Error, Result};

const DEFAULT_BASE_URL: &str = "https://pypi.org";

/// Accurate, user-facing reason surfaced when PyPI's search is walled. A single
/// constant so the `403` and the JS-challenge paths report identically.
const UNAVAILABLE_REASON: &str =
    "PyPI search is unavailable to non-browser clients (no keyless search API; the search page is bot-walled)";
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

        let response = self
            .client
            .get(&url)
            .query(&[("q", q.as_str())])
            .send()
            .await?;

        // A hard 403 is the bot wall's blunt form; a 200 with a challenge-stub
        // body is caught in parse_search_html. Either way it is a persistent wall,
        // not a transient error — surface it as Unavailable (non-retryable). A
        // 429/5xx still flows through error_for_status() as a retryable Http error.
        if response.status() == reqwest::StatusCode::FORBIDDEN {
            return Err(Error::Unavailable(UNAVAILABLE_REASON.to_string()));
        }

        let html = response.error_for_status()?.text().await?;

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

    // Non-trivial page but zero packages parsed: either PyPI's markup drifted or
    // (the current reality) the body is a bot-challenge stub. Indistinguishable
    // from here, and both mean we could not search — surface an accurate,
    // non-retryable Unavailable rather than a silent empty or a misleading
    // "structure may have changed". A small empty page falls through as Ok.
    if matches.is_empty() && html.len() > 2_000 {
        return Err(Error::Unavailable(UNAVAILABLE_REASON.to_string()));
    }

    Ok(matches)
}
