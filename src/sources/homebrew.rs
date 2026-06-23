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
        Self { client, base_url }
    }
}

// 1. We split into three structs to safely handle the different JSON shapes

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
    token: String, // Casks use 'token' for their unique ID
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
        let formula_url = format!("{}/api/formula.json", self.base_url);
        let cask_url = format!("{}/api/cask.json", self.base_url);

        // 1. Fetch and parse Formulae safely
        let formula_res = self
            .client
            .get(&formula_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let formulae: Vec<BrewFormula> = formula_res.error_for_status()?.json().await?;

        // 2. Fetch and parse Casks safely
        let cask_res = self
            .client
            .get(&cask_url)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await?;
        let casks: Vec<BrewCask> = cask_res.error_for_status()?.json().await?;

        // 3. Unify them into a single list of BrewPackages
        let mut packages: Vec<BrewPackage> = formulae
            .into_iter()
            .map(|f| BrewPackage {
                name: f.name,
                desc: f.desc,
                homepage: f.homepage,
            })
            .collect();

        packages.extend(casks.into_iter().map(|c| BrewPackage {
            name: c.token, // Map the cask token to the standard name field
            desc: c.desc,
            homepage: c.homepage,
        }));

        let keywords_lower: Vec<String> = query.keywords.iter().map(|k| k.to_lowercase()).collect();

        // 4. Filter and Map Results
        Ok(packages
            .into_iter()
            .filter(|pkg| {
                let name_lower = pkg.name.to_lowercase();
                let desc_lower = pkg.desc.as_deref().unwrap_or("").to_lowercase();

                // Matches ONLY if every single keyword is found in either the name or description
                keywords_lower
                    .iter()
                    .all(|kw| name_lower.contains(kw) || desc_lower.contains(kw))
            })
            .take(20)
            .map(|pkg| {
                // Synthesize a stable formulae.brew.sh URL when the formula has no
                // homepage, so every row has a unique non-empty URL. Without this,
                // homepage-less formulae all get url="" and the first one silently
                // wins dedup, dropping all the rest.
                let url = pkg
                    .homepage
                    .filter(|h| !h.is_empty())
                    .unwrap_or_else(|| format!("{}/formula/{}", self.base_url, pkg.name));
                Match {
                    name: pkg.name,
                    source: Source::Homebrew,
                    url,
                    description: pkg.desc.unwrap_or_default(),
                    popularity: None,
                    similarity: 0.0,
                }
            })
            .collect())
    }
}
