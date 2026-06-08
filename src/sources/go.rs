//! Go source — scrapes `https://pkg.go.dev/search?q=`.
//!
//! pkg.go.dev has no public JSON search API, so we parse the HTML results page.
//! Brittle by nature — if the markup changes the parse yields nothing, which is
//! treated like any empty result.

use scraper::{Html, Selector};

use super::SourceAdapter;
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

        matches.push(Match {
            name,
            source: Source::Go,
            url,
            description: desc,
            popularity: None,
            similarity: 0.0,
        });

        if matches.len() >= 20 {
            break;
        }
    }

    Ok(matches)
}
