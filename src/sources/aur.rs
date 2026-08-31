//! AUR (Arch User Repository) source.
//!
//! `GET https://aur.archlinux.org/rpc/?v=5&type=search&arg=` returns a clean
//! JSON document `{ resultcount, results: [{ Name, Description, URL, NumVotes,
//! Popularity, … }] }`. The result `URL` field is the upstream project link;
//! the canonical match URL is the AUR package page
//! (`/packages/{Name}`), and the popularity signal is `NumVotes`.
//!
//! The RPC answers **HTTP 200 even when it refuses the query**, flagging the
//! refusal only in the body (`{"type":"error","error":"…","results":[]}`), so
//! the error fields are decoded and acted on: a refusal that reached this
//! adapter as an empty `results` array would be indistinguishable from "the AUR
//! has nothing like this", which is the one claim this tool must never fake.

use serde::Deserialize;

use super::SourceAdapter;
use crate::freshness;
use crate::model::{Match, Query, Source};
use crate::{Error, Result};

/// Default AUR host. Overridable in tests via [`Aur::with_base_url`].
const DEFAULT_BASE_URL: &str = "https://aur.archlinux.org";

/// Match `arg` against package names *and* descriptions. The RPC's default and
/// by far the better recall — `process` finds 1503 packages this way against 78
/// by name alone — but broad terms can exceed the server's result ceiling.
const BY_NAME_DESC: &str = "name-desc";

/// Match `arg` against package names only. Narrow enough to stay under the
/// result ceiling where [`BY_NAME_DESC`] is refused (`port` and `file` are both
/// refused there, and return 636 and 716 hits here), so it serves as a fallback
/// and never as the primary mode.
const BY_NAME: &str = "name";

/// Most packages one narrowing step contributes. A single broad keyword matches
/// hundreds of AUR packages; they arrive unranked, so the tail past the most
/// voted-for handful is noise.
const MAX_HITS: usize = 20;

/// Searches the Arch User Repository.
#[derive(Debug, Clone)]
pub struct Aur {
    client: reqwest::Client,
    base_url: String,
}

impl Aur {
    /// Construct against the live AUR host.
    pub fn new(client: reqwest::Client) -> Self {
        Self::with_base_url(client, DEFAULT_BASE_URL.to_string())
    }

    /// Construct against an arbitrary base URL (used by tests to point at a mock
    /// server). `base_url` should have no trailing slash.
    pub fn with_base_url(client: reqwest::Client, base_url: String) -> Self {
        Self { client, base_url }
    }

    /// One RPC round-trip.
    ///
    /// `Ok(None)` means the RPC refused `arg` as matching too much to return —
    /// the caller can retry it in a narrower match mode. Any other RPC-level
    /// error becomes [`Error::Unavailable`], so the source is reported as "not
    /// reached" instead of passing an empty result off as a clean search.
    async fn rpc(&self, url: &str, arg: &str, by: &str) -> Result<Option<Vec<PackageHit>>> {
        let body: SearchResponse = self
            .client
            .get(url)
            .query(&[("v", "5"), ("type", "search"), ("by", by), ("arg", arg)])
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;

        if body.kind == "error" {
            let reason = body
                .error
                .unwrap_or_else(|| "AUR RPC returned an error".to_string());
            if reason.to_lowercase().contains("too many") {
                return Ok(None);
            }
            return Err(Error::Unavailable(format!("AUR search: {reason}")));
        }
        Ok(Some(body.results))
    }

    /// Keep the most voted-for [`MAX_HITS`] packages and map them into matches.
    fn to_matches(base_url: &str, mut hits: Vec<PackageHit>) -> Vec<Match> {
        hits.sort_by_key(|p| std::cmp::Reverse(p.num_votes.unwrap_or(0)));
        hits.truncate(MAX_HITS);
        hits.into_iter()
            .map(|p| Match {
                url: format!("{}/packages/{}", base_url, p.name),
                name: p.name,
                source: Source::Aur,
                description: p.description.unwrap_or_default(),
                // NumVotes is the AUR's "how established is this" signal. Zero
                // (or absent) votes carries no signal, so it maps to None.
                popularity: p.num_votes.filter(|&v| v > 0),
                similarity: 0.0,
                last_updated: p.last_modified.and_then(freshness::from_unix_secs),
            })
            .collect()
    }
}

/// Top-level shape of the AUR RPC search response. `type` and `error` carry the
/// refusals the HTTP status does not.
#[derive(Debug, Deserialize)]
struct SearchResponse {
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    error: Option<String>,
    #[serde(default)]
    results: Vec<PackageHit>,
}

/// A single package in the `results` array. Only the fields we surface are
/// decoded; the rest of the (large) RPC payload is ignored.
#[derive(Debug, Deserialize)]
struct PackageHit {
    #[serde(rename = "Name")]
    name: String,
    #[serde(rename = "Description", default)]
    description: Option<String>,
    #[serde(rename = "NumVotes", default)]
    num_votes: Option<u64>,
    /// Last change to the package's AUR entry, as epoch **seconds**.
    #[serde(
        rename = "LastModified",
        default,
        deserialize_with = "crate::freshness::lenient"
    )]
    last_modified: Option<i64>,
}

#[async_trait::async_trait]
impl SourceAdapter for Aur {
    fn id(&self) -> Source {
        Source::Aur
    }

    async fn search(&self, query: &Query) -> Result<Vec<Match>> {
        // The RPC matches `arg` as one string against a single package, so a
        // multi-term idea matches nothing: measured live, a five-term idea,
        // "spell syntax mistakes" and "syntax mistakes" all return zero while
        // "mistakes" returns 13. Like Maven and NuGet this one narrows all the
        // way down to a single keyword; the noise that lets in is dropped by
        // similarity ranking.
        let url = format!("{}/rpc/", self.base_url);

        let mut matches = Vec::new();
        for q in super::narrowing_candidates(query, 1) {
            // Narrowing and the result ceiling pull against each other — each
            // step is broader than the last, and broad is what the RPC refuses
            // — so the fallback to name-only matching belongs inside the step,
            // not after the loop.
            let hits = match self.rpc(&url, &q, BY_NAME_DESC).await? {
                Some(hits) => hits,
                None => match self.rpc(&url, &q, BY_NAME).await? {
                    Some(hits) => hits,
                    None => {
                        return Err(Error::Unavailable(
                            "AUR search: query matched too many packages to return".to_string(),
                        ))
                    }
                },
            };

            matches = Self::to_matches(&self.base_url, hits);
            if !matches.is_empty() {
                break;
            }
        }
        Ok(matches)
    }
}
