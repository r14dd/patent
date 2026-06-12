//! Homebrew source — fetches the full catalog from `GET /api/formula.json`
//! and filters it in memory to find matching CLI tools and applications.

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://formulae.brew.sh";

/// Searches Homebrew formulae.
#[derive(Debug, Clone)]
pub struct Homebrew {
    client: reqwest::Client,
    base_url: String,
}

impl Homebrew {
    /// Construct against the live Homebrew API.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
        }
    }

    /// Construct against an arbitrary base URL (used by tests).
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self {
            client,
            base_url,
        }
    }
}

// Note: The Homebrew API returns a flat JSON array of these objects, 
// rather than a nested "items" array like GitHub.
#[derive(Debug, Deserialize)]
struct BrewPackage {
    name: String,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
}


#[async_trait::async_trait]
impl SourceAdapter for Homebrew {
    fn id(&self) -> Source {
        Source::Homebrew
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/api/formula.json", self.base_url);
        
        let search_term = query.keywords.join(" ").to_lowercase();

        let request = self
            .client
            .get(&url)
            .header(reqwest::header::ACCEPT, "application/json");

        let response = request.send().await?;
        let status = response.status();

        if !status.is_success() {
            return Err(crate::Error::Parse(
                format!("Homebrew API returned {} — service might be down", status).into(),
            ));
        }

        let formulae: Vec<BrewPackage> = response.json().await?;

        Ok(formulae
            .into_iter()
            .filter(|pkg| {
                pkg.name.to_lowercase().contains(&search_term)
                    || pkg.desc
                        .as_deref()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&search_term)
            })
            .take(20) // Cap at 20 to match GitHub's per_page limit and avoid UI bloat
            .map(|pkg| Match {
                name: pkg.name,
                source: Source::Homebrew,
                url: pkg.homepage.unwrap_or_default(),
                description: pkg.desc.unwrap_or_default(),
                popularity: None,
                similarity: 0.0,
            })
            .collect())
    }
}