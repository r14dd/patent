//! Hacker News source — `GET https://hn.algolia.com/api/v1/search?query=`
//! (free, no key).

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://hn.algolia.com";
const MAX_DESC_LEN: usize = 120;

/// Searches Hacker News via the Algolia API.
#[derive(Debug, Clone)]
pub struct HackerNews {
    client: reqwest::Client,
    base_url: String,
}

impl HackerNews {
    /// Construct against the live HN Algolia API.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests).
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

#[derive(Debug, Deserialize)]
struct SearchResponse {
    hits: Vec<Hit>,
}

#[derive(Debug, Deserialize)]
struct Hit {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    story_text: Option<String>,
    #[serde(rename = "objectID")]
    object_id: String,
    #[serde(default)]
    points: Option<u64>,
}

fn strip_html_tags(html: &str) -> String {
    let mut result = String::with_capacity(html.len());
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(c),
            _ => {}
        }
    }
    decode_html_entities(result.trim())
}

fn decode_html_entities(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '&' {
            out.push(c);
            continue;
        }
        let mut entity = String::new();
        let mut terminated = false;
        for ec in chars.by_ref() {
            if ec == ';' {
                terminated = true;
                break;
            }
            entity.push(ec);
            if entity.len() > 10 || (!ec.is_alphanumeric() && ec != '#') {
                break;
            }
        }
        if !terminated {
            out.push('&');
            out.push_str(&entity);
            continue;
        }
        match entity.as_str() {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            s if s.starts_with("#x") || s.starts_with("#X") => {
                if let Ok(cp) = u32::from_str_radix(&s[2..], 16) {
                    out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                }
            }
            s if s.starts_with('#') => {
                if let Ok(cp) = s[1..].parse::<u32>() {
                    out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                }
            }
            _ => {
                out.push('&');
                out.push_str(&entity);
                out.push(';');
            }
        }
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let truncated: String = s.chars().take(max).collect();
    format!("{truncated}…")
}

#[async_trait::async_trait]
impl SourceAdapter for HackerNews {
    fn id(&self) -> Source {
        Source::HackerNews
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/api/v1/search", self.base_url);
        let q = query.idea.clone();

        let body: SearchResponse = self
            .client
            .get(&url)
            .query(&[
                ("query", q.as_str()),
                ("hitsPerPage", "20"),
                ("tags", "story"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(body
            .hits
            .into_iter()
            .filter(|h| h.title.as_ref().is_some_and(|t| !t.is_empty()))
            .map(|h| {
                let title = h.title.unwrap_or_default();
                let desc = h
                    .story_text
                    .as_deref()
                    .map(strip_html_tags)
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| title.clone());
                let desc = truncate(&desc, MAX_DESC_LEN);
                Match {
                    name: title,
                    source: Source::HackerNews,
                    url: format!("https://news.ycombinator.com/item?id={}", h.object_id),
                    description: desc,
                    popularity: h.points,
                    similarity: 0.0,
                    last_updated: None,
                }
            })
            .collect())
    }
}
