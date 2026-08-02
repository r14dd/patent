//! Go source — scrapes `https://pkg.go.dev/search?q=`.
//!
//! pkg.go.dev has no public JSON search API, so we parse the HTML results page.
//! Brittle by nature — if the markup changes the parse yields nothing, which is
//! treated like any empty result.

use scraper::{Html, Selector};

use super::SourceAdapter;
use crate::freshness;
use crate::model::{Match, Query, Source};
use crate::{Error, Result};

const DEFAULT_BASE_URL: &str = "https://pkg.go.dev";
#[derive(Debug, Clone)]
pub struct GoPkgDev {
    client: reqwest::Client,
    base_url: String,
}

impl GoPkgDev {
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[async_trait::async_trait]
impl SourceAdapter for GoPkgDev {
    fn id(&self) -> Source {
        Source::Go
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/search", self.base_url);
        let q = query.keywords.join(" ");

        let html = self
            .client
            .get(&url)
            .query(&[("q", q.as_str()), ("m", "package")])
            .send()
            .await?
            .error_for_status()?
            .text()
            .await?;

        parse_search_html(&html, &self.base_url)
    }
}

fn parse_search_html(html: &str, base_url: &str) -> Result<Vec<Match>> {
    let snippet = Selector::parse(".SearchSnippet")
        .map_err(|e| Error::Parse(format!("bad selector: {e}")))?;
    let header =
        Selector::parse("a[href]").map_err(|e| Error::Parse(format!("bad selector: {e}")))?;
    let synopsis = Selector::parse(".SearchSnippet-synopsis")
        .map_err(|e| Error::Parse(format!("bad selector: {e}")))?;
    // The only source whose date is scraped rather than read from an API field.
    // A missing or reworded element just leaves `last_updated` empty — unlike
    // the name/URL parse above, it never counts as drift worth failing over.
    let published = Selector::parse("[data-test-id='snippet-published']")
        .map_err(|e| Error::Parse(format!("bad selector: {e}")))?;

    let document = Html::parse_document(html);
    let mut matches = Vec::new();

    for element in document.select(&snippet) {
        let Some(link) = element.select(&header).next() else {
            continue;
        };
        let href = link.value().attr("href").unwrap_or("");
        let name = link.text().collect::<String>().trim().to_string();
        if name.is_empty() {
            continue;
        }

        let desc = element
            .select(&synopsis)
            .next()
            .map(|s| s.text().collect::<String>().trim().to_string())
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| name.clone());

        let url = if href.starts_with("http") {
            href.to_string()
        } else {
            format!("{base_url}{href}")
        };

        // Rendered as a human date, e.g. "Feb 28, 2026".
        let last_updated = element
            .select(&published)
            .next()
            .map(|s| s.text().collect::<String>())
            .and_then(|s| freshness::from_go_date(&s));

        matches.push(Match {
            name,
            source: Source::Go,
            url,
            description: desc,
            popularity: None,
            similarity: 0.0,
            last_updated,
        });

        if matches.len() >= 20 {
            break;
        }
    }

    // Drift detection: if the page looks non-trivial but we parsed nothing,
    // the markup probably changed. Signal the retry path instead of silently
    // returning empty -- an empty result from a real page is misleading.
    if matches.is_empty() && html.len() > 2_000 {
        return Err(Error::Parse(
            "pkg.go.dev search page structure may have changed -- zero packages parsed from a non-trivial response".into(),
        ));
    }

    Ok(matches)
}
