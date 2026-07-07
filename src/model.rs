//! Core domain types shared across the pipeline.
//!
//! These are deliberately small and `serde`-serializable so the `--json` path
//! and the TUI render from the same data.

use serde::{Deserialize, Serialize};

/// A user's idea, plus keywords derived from it for keyword-based source APIs.
///
/// The full `idea` string is what the embedder ranks against; `keywords` is the
/// cleaned-up query handed to registry search endpoints.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub idea: String,
    pub keywords: Vec<String>,
}

/// Where a [`Match`] came from. Always surfaced to the user for transparency.
///
/// Serializes to stable kebab-case identifiers (e.g. `"crates-io"`, not the Rust
/// variant name). Old PascalCase names are accepted on deserialization for backward
/// compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    #[serde(rename = "crates-io", alias = "CratesIo")]
    CratesIo,
    #[serde(rename = "github", alias = "GitHub")]
    GitHub,
    #[serde(rename = "npm", alias = "Npm")]
    Npm,
    #[serde(rename = "pypi", alias = "PyPI")]
    PyPI,
    #[serde(rename = "hacker-news", alias = "HackerNews")]
    HackerNews,
    #[serde(rename = "go", alias = "Go")]
    Go,
    #[serde(rename = "maven", alias = "Maven")]
    Maven,
    #[serde(rename = "rubygems", alias = "RubyGems")]
    RubyGems,
    #[serde(rename = "docker-hub", alias = "DockerHub")]
    DockerHub,
    #[serde(rename = "vscode-marketplace", alias = "VsCodeMarketplace")]
    VsCodeMarketplace,
    #[serde(rename = "nuget", alias = "NuGet")]
    NuGet,
    #[serde(rename = "homebrew", alias = "Homebrew")]
    Homebrew,
    #[serde(rename = "packagist", alias = "Packagist")]
    Packagist,
    #[serde(rename = "hex", alias = "Hex")]
    Hex,
    #[serde(rename = "artifact-hub", alias = "ArtifactHub")]
    ArtifactHub,
    #[serde(rename = "aur", alias = "Aur")]
    Aur,
    #[serde(rename = "hackage", alias = "Hackage")]
    Hackage,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::CratesIo => f.write_str("crates.io"),
            Self::GitHub => f.write_str("GitHub"),
            Self::Npm => f.write_str("npm"),
            Self::PyPI => f.write_str("PyPI"),
            Self::HackerNews => f.write_str("Hacker News"),
            Self::Go => f.write_str("Go"),
            Self::Maven => f.write_str("Maven"),
            Self::RubyGems => f.write_str("RubyGems"),
            Self::DockerHub => f.write_str("Docker Hub"),
            Self::VsCodeMarketplace => f.write_str("VS Code"),
            Self::NuGet => f.write_str("NuGet"),
            Self::Homebrew => f.write_str("Homebrew"),
            Self::Packagist => f.write_str("Packagist"),
            Self::Hex => f.write_str("Hex"),
            Self::ArtifactHub => f.write_str("Artifact Hub"),
            Self::Aur => f.write_str("AUR"),
            Self::Hackage => f.write_str("Hackage"),
        }
    }
}

/// A single piece of prior art found in a [`Source`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Match {
    pub name: String,
    pub source: Source,
    pub url: String,
    pub description: String,
    /// Source-specific popularity signal (downloads, stars, points…). Optional
    /// because not every source exposes one.
    pub popularity: Option<u64>,
    /// Cosine similarity to the idea, in `[0.0, 1.0]`. Filled in by `rank`.
    pub similarity: f32,
}

/// How crowded the space looks, based on what was found in the sources checked.
///
/// Ordered `Open < Crowded < Saturated` so the verdict level can be *floored*
/// against the similarity data (the model is never allowed to under-rate a
/// space that the embeddings show is clearly populated).
///
/// Serializes to lowercase (`"open"`, `"crowded"`, `"saturated"`). Old PascalCase
/// names are accepted on deserialization for backward compatibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Saturation {
    /// 🟢 nothing close found in the sources checked.
    #[serde(alias = "Open")]
    Open,
    /// 🟡 a few adjacent things exist.
    #[serde(alias = "Crowded")]
    Crowded,
    /// 🔴 the space is densely populated.
    #[serde(alias = "Saturated")]
    Saturated,
}

impl Saturation {
    pub fn exit_code(self) -> i32 {
        match self {
            Self::Open => 0,
            Self::Crowded => 1,
            Self::Saturated => 2,
        }
    }
}

impl std::fmt::Display for Saturation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => f.write_str("Open"),
            Self::Crowded => f.write_str("Crowded"),
            Self::Saturated => f.write_str("Saturated"),
        }
    }
}

/// The model-written, integrity-scoped verdict.
///
/// Invariant: copy is always phrased as "found in the sources checked" and never
/// asserts that something does not exist anywhere.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Verdict {
    pub level: Saturation,
    pub headline: String,
    pub gaps: Vec<String>,
    /// Sources that were searched successfully — always surfaced for transparency.
    pub sources_checked: Vec<Source>,
    /// Sources that were selected but failed to respond (e.g. network error or
    /// rate limit). Surfaced so a thin or empty result isn't mistaken for
    /// "nothing out there" when coverage was actually reduced.
    #[serde(default)]
    pub sources_failed: Vec<Source>,
    pub caveat: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_code_graduated() {
        assert_eq!(Saturation::Open.exit_code(), 0);
        assert_eq!(Saturation::Crowded.exit_code(), 1);
        assert_eq!(Saturation::Saturated.exit_code(), 2);
    }
}
