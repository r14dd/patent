//! Source registry: one implementor per ecosystem, fanned out concurrently.
//!
//! Sources are selected based on the query — a Rust query searches crates.io,
//! a Python query searches PyPI, etc. GitHub is always included. When no
//! language is detected, the three largest registries (npm, PyPI, crates.io)
//! are used as a broad fallback.

use std::collections::HashSet;
use std::time::Duration;

use futures::future::join_all;

use crate::model::{Match, Query};
use crate::Result;

pub mod crates_io;
pub mod docker_hub;
pub mod github;
pub mod go;
pub mod hacker_news;
pub mod homebrew;
pub mod maven;
pub mod npm;
pub mod nuget;
pub mod pypi;
pub mod rubygems;
pub mod vscode;

/// One searchable ecosystem (a registry, a forge, a community index).
#[async_trait::async_trait]
pub trait SourceAdapter: Send + Sync {
    /// Stable identifier, used in the transparency line.
    fn id(&self) -> crate::model::Source;

    /// Search this source for prior art matching `query`.
    async fn search(&self, query: &Query) -> Result<Vec<Match>>;
}

use crate::model::Source as S;

fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!(
            "patent/",
            env!("CARGO_PKG_VERSION"),
            " (prior-art search; https://github.com/r14dd/patent)"
        ))
        .build()
        .expect("failed to build HTTP client")
}

fn idea_contains(idea: &str, terms: &[&str]) -> bool {
    let lower = idea.to_lowercase();
    let bytes = lower.as_bytes();
    terms.iter().any(|t| {
        // Check EVERY occurrence, not just the first: a short keyword like "go"
        // may first appear inside a larger word ("django") and then again as a
        // standalone word — only the standalone one should count.
        lower.match_indices(t).any(|(pos, _)| {
            let before = pos == 0 || !bytes[pos - 1].is_ascii_alphanumeric();
            let after_pos = pos + t.len();
            let after = after_pos >= bytes.len() || !bytes[after_pos].is_ascii_alphanumeric();
            before && after
        })
    })
}

fn add(set: &mut HashSet<S>, sources: &[S]) {
    set.extend(sources);
}

fn detect_sources(idea: &str) -> HashSet<S> {
    let mut s = HashSet::new();

    // GitHub and Hacker News are always included: GitHub is the cross-language
    // home of source code, and Hacker News is the cross-language home of the
    // "Show HN" launch / discussion that often predates a registry release.
    // Both are language-agnostic, so they apply to every query.
    s.insert(S::GitHub);
    s.insert(S::HackerNews);

    // ── Explicit language / ecosystem mentions ──────────────────────────
    if idea_contains(idea, &["rust", "crate", "cargo"]) {
        s.insert(S::CratesIo);
    }
    if idea_contains(idea, &["brew", "homebrew", "macos", "cask"]) {
        s.insert(S::Homebrew);
    }
    if idea_contains(
        idea,
        &["npm", "node", "javascript", "typescript", "deno", "bun"],
    ) {
        s.insert(S::Npm);
    }
    if idea_contains(
        idea,
        &["python", "pip", "django", "flask", "pytorch", "pandas"],
    ) {
        s.insert(S::PyPI);
    }
    if idea_contains(idea, &["go", "golang", "goroutine"]) {
        s.insert(S::Go);
    }
    if idea_contains(
        idea,
        &["java", "kotlin", "spring", "maven", "gradle", "scala"],
    ) {
        s.insert(S::Maven);
    }
    if idea_contains(idea, &["ruby", "rails", "sinatra", "gem"]) {
        s.insert(S::RubyGems);
    }
    if idea_contains(
        idea,
        &["c#", ".net", "csharp", "dotnet", "nuget", "blazor", "unity"],
    ) {
        s.insert(S::NuGet);
    }

    // ── Domain inference (no language named, but the problem implies one) ─
    if idea_contains(idea, &["cli", "command line", "terminal tool", "shell"]) {
        add(&mut s, &[S::CratesIo, S::Go, S::Npm, S::PyPI, S::Homebrew]);
    }
    if idea_contains(
        idea,
        &[
            "ai",
            "llm",
            "machine learning",
            "deep learning",
            "neural",
            "model training",
            "inference",
            "embedding",
            "nlp",
            "computer vision",
            "data science",
            "data pipeline",
        ],
    ) {
        add(&mut s, &[S::PyPI, S::Npm]);
    }
    // CLI tools span every ecosystem, so a generic "cli" mention casts the net
    // across the dominant registries rather than just Rust/Go — otherwise the
    // flagship "kill the process on a port" demo would never search npm, where
    // fkill-cli / kill-port actually live.
    if idea_contains(idea, &["cli", "command line", "terminal tool", "shell"]) {
        add(&mut s, &[S::CratesIo, S::Go, S::Npm, S::PyPI]);
    }
    if idea_contains(
        idea,
        &[
            "frontend",
            "react",
            "vue",
            "angular",
            "svelte",
            "browser",
            "css",
            "ui component",
            "web component",
            "spa",
        ],
    ) {
        s.insert(S::Npm);
    }
    if idea_contains(
        idea,
        &[
            "api",
            "backend",
            "rest",
            "graphql",
            "microservice",
            "web server",
        ],
    ) {
        add(&mut s, &[S::Npm, S::PyPI, S::Go]);
    }
    if idea_contains(
        idea,
        &[
            "mobile",
            "ios",
            "android",
            "react native",
            "flutter",
            "swift",
            "swiftui",
        ],
    ) {
        add(&mut s, &[S::Npm, S::Maven]);
    }
    if idea_contains(
        idea,
        &[
            "game",
            "graphics",
            "rendering",
            "opengl",
            "vulkan",
            "bevy",
            "godot",
        ],
    ) {
        add(&mut s, &[S::CratesIo, S::NuGet]);
    }
    if idea_contains(idea, &["embedded", "firmware", "microcontroller", "rtos"]) {
        s.insert(S::CratesIo);
    }
    if idea_contains(
        idea,
        &[
            "docker",
            "container",
            "kubernetes",
            "k8s",
            "helm",
            "deploy",
            "infrastructure",
        ],
    ) {
        add(&mut s, &[S::DockerHub, S::Go]);
    }
    if idea_contains(idea, &["vscode", "extension", "plugin", "ide", "editor"]) {
        add(&mut s, &[S::VsCodeMarketplace, S::Npm]);
    }

    // ── Fallback: no signal at all → broad sweep ────────────────────────
    // The always-on GitHub + Hacker News aren't enough on their own; if no
    // language/domain branch matched, add the 3 biggest registries.
    const ALWAYS_ON: usize = 2; // GitHub + Hacker News
    if s.len() <= ALWAYS_ON {
        add(&mut s, &[S::Npm, S::PyPI, S::CratesIo]);
    }

    s
}

fn build_source(id: S, client: reqwest::Client) -> Box<dyn SourceAdapter> {
    match id {
        S::CratesIo => Box::new(crates_io::CratesIo::new(client)),
        S::GitHub => Box::new(github::GitHub::new(client)),
        S::Npm => Box::new(npm::Npm::new(client)),
        S::PyPI => Box::new(pypi::PyPI::new(client)),
        S::HackerNews => Box::new(hacker_news::HackerNews::new(client)),
        S::Go => Box::new(go::GoPkgDev::new(client)),
        S::Maven => Box::new(maven::Maven::new(client)),
        S::RubyGems => Box::new(rubygems::RubyGems::new(client)),
        S::DockerHub => Box::new(docker_hub::DockerHub::new(client)),
        S::VsCodeMarketplace => Box::new(vscode::VsCodeMarketplace::new(client)),
        S::NuGet => Box::new(nuget::NuGet::new(client)),
        S::Homebrew => Box::new(homebrew::Homebrew::new(client)),
    }
}

/// Pick sources based on what the query is about.
fn sources_for(query: &Query) -> Vec<Box<dyn SourceAdapter>> {
    let client = http_client();
    let ids = detect_sources(&query.idea);
    ids.into_iter()
        .map(|id| build_source(id, client.clone()))
        .collect()
}

/// The outcome of a fan-out: deduped matches, the sources that responded, and
/// the selected sources that failed (so reduced coverage can be surfaced).
pub struct SearchOutcome {
    pub matches: Vec<Match>,
    pub reached: Vec<crate::model::Source>,
    pub failed: Vec<crate::model::Source>,
}

/// Fan out to selected sources concurrently, dropping the ones that fail.
pub async fn search_all(query: &Query) -> SearchOutcome {
    search_sources(&sources_for(query), query).await
}

/// Run `query` against `sources` concurrently, skipping any that error, and
/// dedup the combined results. Returns the deduped matches, which sources
/// responded successfully, and which were attempted but failed. Exposed for
/// testing the fan-out in isolation.
pub async fn search_sources(sources: &[Box<dyn SourceAdapter>], query: &Query) -> SearchOutcome {
    let results = join_all(sources.iter().map(|s| {
        let id = s.id();
        async move {
            let first = s.search(query).await;
            if first.is_ok() {
                return (id, first);
            }
            tokio::time::sleep(Duration::from_millis(800)).await;
            (id, s.search(query).await)
        }
    }))
    .await;

    let mut reached = Vec::new();
    let mut failed = Vec::new();
    let mut all = Vec::new();
    for (id, result) in results {
        match result {
            Ok(matches) => {
                reached.push(id);
                all.extend(matches);
            }
            Err(e) => {
                eprintln!("⚠  {id} not reached: {e}");
                failed.push(id);
            }
        }
    }
    SearchOutcome {
        matches: dedup(all),
        reached,
        failed,
    }
}

/// Remove duplicate matches by URL, keeping the first occurrence and preserving
/// order. URL is a match's canonical identity across sources.
pub fn dedup(matches: Vec<Match>) -> Vec<Match> {
    let mut seen = HashSet::new();
    matches
        .into_iter()
        .filter(|m| seen.insert(m.url.clone()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idea_contains_respects_word_boundaries() {
        assert!(idea_contains("a fast async runtime", &["async"]));
        // Substrings inside larger words must not match.
        assert!(!idea_contains("rainbow trains", &["ai"]));
        assert!(!idea_contains("googol", &["go"]));
        assert!(!idea_contains("django framework", &["go"]));
    }

    #[test]
    fn idea_contains_checks_all_occurrences_not_just_the_first() {
        // A non-boundary substring earlier in the string must not mask a later
        // standalone occurrence of the keyword.
        assert!(idea_contains(
            "a tool for cargo packages written in go",
            &["go"]
        ));
        assert!(idea_contains("email summarizer that uses ai", &["ai"]));
        assert!(idea_contains("a good way to go fast", &["go"]));
    }

    #[test]
    fn github_and_hacker_news_are_always_selected() {
        // Whatever the idea, the two language-agnostic indexes are present so
        // they are never falsely advertised but unreachable.
        for idea in ["a ruby gem for parsing csv", "asdf qwer zxcv", "rust crate"] {
            let s = detect_sources(idea);
            assert!(s.contains(&S::GitHub), "GitHub missing for {idea:?}");
            assert!(
                s.contains(&S::HackerNews),
                "Hacker News missing for {idea:?}"
            );
        }
    }

    #[test]
    fn every_built_source_is_reachable_from_some_idea() {
        // Guards against re-introducing a "marketed but never selected" source:
        // each variant build_source can construct must be selectable.
        let ideas = [
            "rust crate for embedded firmware",
            "a python pandas data pipeline",
            "a typescript react frontend component",
            "a golang microservice",
            "a java spring boot service",
            "a ruby on rails gem",
            "a c# dotnet unity game",
            "a docker container for kubernetes",
            "a vscode extension for editors",
            "a macos homebrew tool",
            "anything at all with no signal",
        ];
        let mut seen: HashSet<S> = HashSet::new();
        for idea in ideas {
            seen.extend(detect_sources(idea));
        }
        for variant in [
            S::CratesIo,
            S::GitHub,
            S::Npm,
            S::PyPI,
            S::HackerNews,
            S::Go,
            S::Maven,
            S::RubyGems,
            S::DockerHub,
            S::VsCodeMarketplace,
            S::NuGet,
            S::Homebrew,
        ] {
            assert!(
                seen.contains(&variant),
                "{variant} is built but never selected by detect_sources"
            );
        }
    }

    #[test]
    fn language_mentions_select_their_registry() {
        assert!(detect_sources("a rust crate for parsing").contains(&S::CratesIo));
        assert!(detect_sources("a python library for parsing").contains(&S::PyPI));
        assert!(detect_sources("a docker image for caching").contains(&S::DockerHub));
        assert!(detect_sources("a ruby gem for parsing").contains(&S::RubyGems));
    }

    #[test]
    fn go_and_ai_match_natural_phrasings() {
        // Regression: trailing-space keywords ("go ", "ai ") used to be
        // impossible to match. These phrasings deliberately avoid the "cli"
        // branch (which would add Go on its own) so they isolate the keyword.
        assert!(detect_sources("a fast Go library for parsing json").contains(&S::Go));
        assert!(detect_sources("a library that uses AI to summarize text").contains(&S::PyPI));
        // And the keyword must still win when a non-boundary substring precedes
        // the standalone word (the first-occurrence regression).
        assert!(detect_sources("a cargo workspace tool also written in go").contains(&S::Go));
    }

    #[test]
    fn port_killer_demo_searches_npm() {
        // The flagship README example must reach npm, where fkill-cli /
        // kill-port live — otherwise the headline demo finds no prior art.
        for idea in [
            "interactive cli to kill whatever's on a port",
            "CLI tool that kills whatever's on a port",
        ] {
            let s = detect_sources(idea);
            assert!(s.contains(&S::Npm), "npm missing for {idea:?}: {s:?}");
        }
    }

    #[test]
    fn no_signal_falls_back_to_broad_sweep() {
        let s = detect_sources("asdf qwer zxcv hjkl");
        assert!(s.contains(&S::Npm));
        assert!(s.contains(&S::PyPI));
        assert!(s.contains(&S::CratesIo));
    }
}
