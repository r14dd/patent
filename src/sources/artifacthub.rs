//! Artifact Hub source — `GET https://artifacthub.io/api/v1/packages/search`.
//!
//! Artifact Hub indexes Cloud Native packages (Helm charts, OLM operators, Krew
//! kubectl plugins, container images, Kyverno/OPA policies, …). The search API
//! returns clean JSON, so this mirrors the crates.io / npm adapters: hit the
//! endpoint, `error_for_status`, then `json`.
//!
//! A package's stable web URL is `…/packages/{kind}/{repo}/{name}`, where
//! `{kind}` is a slug (e.g. `helm`, `olm`, `krew`) — but the API only exposes a
//! numeric `repository.kind`, so [`kind_slug`] maps the number to the slug used
//! in the path.

use serde::Deserialize;

use super::SourceAdapter;
use crate::model::{Match, Query, Source};
use crate::Result;

/// Default Artifact Hub host. Overridable in tests via [`ArtifactHub::with_base_url`].
const DEFAULT_BASE_URL: &str = "https://artifacthub.io";

/// Searches the Artifact Hub package index.
#[derive(Debug, Clone)]
pub struct ArtifactHub {
    client: reqwest::Client,
    base_url: String,
}

impl ArtifactHub {
    /// Construct against the live Artifact Hub host.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests to point at a mock
    /// server). `base_url` should have no trailing slash.
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }
}

/// Top-level shape of the Artifact Hub search response.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(default)]
    packages: Vec<PackageHit>,
}

/// A single package in the `packages` array. Only the fields we surface are decoded.
#[derive(Debug, Deserialize)]
struct PackageHit {
    name: String,
    #[serde(default)]
    description: Option<String>,
    /// Stargazers on Artifact Hub itself; used as the popularity signal.
    #[serde(default)]
    stars: Option<u64>,
    repository: Repository,
}

/// The repository a package belongs to. `name` and `kind` build the web URL;
/// `url` is the fallback link for kinds this adapter doesn't yet know a slug for.
#[derive(Debug, Deserialize)]
struct Repository {
    name: String,
    /// Numeric repository kind; mapped to a URL slug by [`kind_slug`].
    kind: u32,
    #[serde(default)]
    url: Option<String>,
}

/// Map Artifact Hub's numeric `repository.kind` to the slug used in package web
/// URLs (`…/packages/{slug}/{repo}/{name}`). Returns `None` for kinds added
/// after this mapping was written, so the caller can fall back gracefully rather
/// than emit a broken link.
fn kind_slug(kind: u32) -> Option<&'static str> {
    Some(match kind {
        0 => "helm",
        1 => "falco",
        2 => "opa",
        3 => "olm",
        4 => "tbaction",
        5 => "krew",
        6 => "helm-plugin",
        7 => "tekton-task",
        8 => "keda-scaler",
        9 => "coredns",
        10 => "keptn",
        11 => "tekton-pipeline",
        12 => "container",
        13 => "kubewarden",
        14 => "gatekeeper",
        15 => "kyverno",
        16 => "knative-client-plugin",
        17 => "backstage",
        18 => "argo-template",
        19 => "kubearmor",
        20 => "kcl",
        21 => "headlamp",
        22 => "inspektor-gadget",
        23 => "tekton-stepaction",
        24 => "meshery",
        25 => "opencost",
        26 => "radius",
        27 => "bootc",
        _ => return None,
    })
}

/// Build a package's web URL. For a known kind this is the canonical Artifact Hub
/// package page; for an unknown (future) kind we fall back to the repository's
/// own URL so the link is still useful rather than a guaranteed 404.
fn package_url(base_url: &str, repo: &Repository, name: &str) -> String {
    match kind_slug(repo.kind) {
        Some(slug) => format!("{base_url}/packages/{slug}/{}/{name}", repo.name),
        None => repo.url.clone().unwrap_or_default(),
    }
}

#[async_trait::async_trait]
impl SourceAdapter for ArtifactHub {
    fn id(&self) -> Source {
        Source::ArtifactHub
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        let url = format!("{}/api/v1/packages/search", self.base_url);
        let q = query.keywords.join(" ");

        let body: SearchResponse = self
            .client
            .get(&url)
            .query(&[
                ("ts_query_web", q.as_str()),
                ("limit", "15"),
                ("facets", "false"),
            ])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        Ok(body
            .packages
            .into_iter()
            .map(|p| {
                let url = package_url(&self.base_url, &p.repository, &p.name);
                Match {
                    url,
                    name: p.name,
                    source: Source::ArtifactHub,
                    description: p.description.unwrap_or_default(),
                    // None if absent or zero — a 0-star package is not a
                    // meaningful popularity signal.
                    popularity: p.stars.filter(|&s| s > 0),
                    similarity: 0.0,
                }
            })
            .collect())
    }
}
