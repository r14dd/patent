//! Homebrew source — fetches the full catalog from `GET /api/formula.json`
//! and filters it in memory to find matching CLI tools and applications.

use std::sync::Arc;

use serde::Deserialize;
use tokio::sync::OnceCell;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

const DEFAULT_BASE_URL: &str = "https://formulae.brew.sh";

static SHARED_CATALOG: std::sync::OnceLock<Arc<OnceCell<Vec<BrewPackage>>>> =
    std::sync::OnceLock::new();

/// Searches Homebrew formulae.
#[derive(Debug, Clone)]
pub struct Homebrew {
    client: reqwest::Client,
    base_url: String,
    catalog: Arc<OnceCell<Vec<BrewPackage>>>,
}

impl Homebrew {
    /// Construct against the live Homebrew API.
    pub fn new(client: reqwest::Client) -> Self {
        let catalog = SHARED_CATALOG
            .get_or_init(|| Arc::new(OnceCell::new()))
            .clone();
        Self {
            client,
            base_url: DEFAULT_BASE_URL.to_string(),
            catalog,
        }
    }

    /// Construct against an arbitrary base URL (used by tests).
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self {
            client,
            base_url,
            catalog: Arc::new(OnceCell::new()),
        }
    }

    async fn fetch_catalog(&self) -> Result<Vec<BrewPackage>> {
        let formula_url = format!("{}/api/formula.json", self.base_url);
        let cask_url = format!("{}/api/cask.json", self.base_url);

        let formula_res = self
            .client
            .get(&formula_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let formulae: Vec<BrewFormula> = formula_res.error_for_status()?.json().await?;

        let cask_res = self
            .client
            .get(&cask_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let casks: Vec<BrewCask> = cask_res.error_for_status()?.json().await?;

        let mut packages: Vec<BrewPackage> = formulae
            .into_iter()
            .map(|f| BrewPackage {
                name: f.name,
                desc: f.desc,
                homepage: f.homepage,
            })
            .collect();

        packages.extend(casks.into_iter().map(|c| BrewPackage {
            name: c.token,
            desc: c.desc,
            homepage: c.homepage,
        }));

        Ok(packages)
    }
}

#[derive(Debug)]
struct BrewPackage {
    name: String,
    desc: Option<String>,
    homepage: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrewFormula {
    name: String,
    #[serde(default)]
    desc: Option<String>,
    #[serde(default)]
    homepage: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrewCask {
    token: String,
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
        let packages = self
            .catalog
            .get_or_try_init(|| self.fetch_catalog())
            .await?;

        // Every keyword has to appear in the formula's name or one-line
        // description, and those descriptions are short: a realistic 7-term
        // idea matches nothing at all, while its 2 longest terms match real
        // formulae. Narrow progressively until something matches. The catalog
        // is already cached in memory, so the extra passes cost no requests.
        let mut matches = Vec::new();
        for candidate in super::narrowing_candidates(query, 2) {
            let terms: Vec<String> = candidate
                .split_whitespace()
                .map(|k| k.to_lowercase())
                .collect();

            matches = packages
                .iter()
                .filter(|pkg| {
                    let name_lower = pkg.name.to_lowercase();
                    let desc_lower = pkg.desc.as_deref().unwrap_or("").to_lowercase();
                    terms
                        .iter()
                        .all(|kw| name_lower.contains(kw) || desc_lower.contains(kw))
                })
                .take(20)
                .map(|pkg| {
                    let url = pkg
                        .homepage
                        .as_ref()
                        .filter(|h| !h.is_empty())
                        .cloned()
                        .unwrap_or_else(|| format!("{}/formula/{}", self.base_url, pkg.name));
                    Match {
                        name: pkg.name.clone(),
                        source: Source::Homebrew,
                        url,
                        description: pkg.desc.clone().unwrap_or_default(),
                        popularity: None,
                        similarity: 0.0,
                        last_updated: None,
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
