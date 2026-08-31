//! Source registry: one implementor per ecosystem, fanned out concurrently.
//!
//! Sources are selected based on the query — a Rust query searches crates.io,
//! a Python query searches PyPI, etc. GitHub is always included. When no
//! language is detected, the three largest registries (npm, PyPI, crates.io)
//! are used as a broad fallback.
//!
//! # Which sources report a last-updated date
//!
//! [`Match::last_updated`] is populated only where the *search* response
//! already carries a date. Fetching one per result would mean an extra request
//! per match, and with `SOURCE_TIMEOUT` bounding the whole source that is not
//! a trade worth making. The split below was established by probing each live
//! API, so the gaps are deliberate rather than unfinished:
//!
//! | Populated | Field |
//! |---|---|
//! | crates.io | `updated_at` |
//! | npm | `package.date` |
//! | GitHub | `pushed_at` (last push, not metadata edits) |
//! | Hex | `updated_at` |
//! | Maven | `timestamp` (epoch millis) |
//! | AUR | `LastModified` (epoch secs) |
//! | Artifact Hub | `ts` (epoch secs) |
//! | Go | scraped from the rendered "published on" date |
//! | JetBrains Marketplace | `cdate` (epoch millis, last-updated not creation) |
//!
//! Always `None`: **RubyGems**, **Docker Hub**, **NuGet**, **Packagist**,
//! **Homebrew**, **Hackage**, **Nixpkgs** and **VS Code Marketplace** return
//! no date in their search responses at all; **PyPI** is bot-walled and
//! returns nothing to parse.
//! **Hacker News** does expose `created_at`, but that is when a thread was
//! posted — it says nothing about whether the thing discussed is maintained,
//! and rendering a 2012 discussion as "stale" would be actively misleading.
//!
//! The table describes what each source *publishes*, not what every rendered
//! row ends up carrying. A match attributed to an undated source can still show
//! a date if another source returned the same URL: [`dedup`] merges duplicates
//! and fills the kept match's gaps. A Homebrew formula whose homepage is a
//! GitHub repo inherits that repo's `pushed_at` — a date about the same
//! artifact, from the source that does publish one.
//!
//! [`Match::last_updated`]: crate::model::Match::last_updated

use std::collections::{HashMap, HashSet};
use std::time::Duration;

use futures::future::join_all;

use crate::model::{Match, Query};
use crate::Result;

pub mod artifacthub;
pub mod aur;
pub mod crates_io;
pub mod docker_hub;
pub mod github;
pub mod go;
pub mod hackage;
pub mod hacker_news;
pub mod hex;
pub mod homebrew;
pub mod jetbrains;
pub mod maven;
pub mod nixpkgs;
pub mod npm;
pub mod nuget;
pub mod packagist;
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

/// Picks the `n` longest keywords, preserving their original order (not sorted
/// by length): the longest terms carry the most meaning, but reordering them
/// changes what a relevance-ranked registry returns.
fn narrowed(keywords: &[String], n: usize) -> String {
    let mut idx: Vec<usize> = (0..keywords.len()).collect();
    idx.sort_by(|&a, &b| keywords[b].len().cmp(&keywords[a].len()));
    idx.truncate(n);
    idx.sort_unstable();
    idx.into_iter()
        .map(|i| keywords[i].as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Query strings to try, in order, against a source that ANDs its search terms.
///
/// Such a source returns nothing for a realistic multi-keyword idea even where
/// matching packages exist — measured live, a 7-term idea returns zero from
/// half the registries while 2-3 of its longest terms return hundreds. Callers
/// try each candidate until one comes back non-empty.
///
/// The list runs from the full keyword set down to `min_terms` of the longest
/// keywords; duplicates (a narrowing that repeats a string already tried) are
/// dropped, and a query with no keywords at all falls back to the raw idea
/// rather than sending an empty search param. `min_terms` is per-source and
/// measured, not guessed: most sources recover at 2, while Maven, NuGet and
/// Artifact Hub index strictly enough to need a single keyword.
pub(crate) fn narrowing_candidates(query: &Query, min_terms: usize) -> Vec<String> {
    let mut candidates = Vec::new();
    let steps = std::iter::once(query.keywords.len()).chain((min_terms..=3).rev());
    for n in steps {
        let q = narrowed(&query.keywords, n);
        if !q.is_empty() && !candidates.contains(&q) {
            candidates.push(q);
        }
    }
    if candidates.is_empty() {
        candidates.push(query.idea.clone());
    }
    candidates
}

use crate::model::Source as S;

fn http_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .connect_timeout(Duration::from_secs(5))
        .user_agent(concat!(
            "patent/",
            env!("CARGO_PKG_VERSION"),
            " (prior-art search; https://github.com/r14dd/patent)"
        ))
        .build()
        .map_err(crate::Error::HttpClient)
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
    if idea_contains(idea, &["php", "composer", "laravel", "symfony"]) {
        s.insert(S::Packagist);
    }
    if idea_contains(idea, &["elixir", "erlang", "phoenix", "hex", "mix"]) {
        s.insert(S::Hex);
    }
    if idea_contains(
        idea,
        &[
            "helm",
            "kubernetes",
            "k8s",
            "cncf",
            "cloud-native",
            "operator",
            "kubectl",
            "crd",
        ],
    ) {
        s.insert(S::ArtifactHub);
    }
    if idea_contains(idea, &["arch", "aur", "pacman", "archlinux"]) {
        s.insert(S::Aur);
    }
    if idea_contains(idea, &["haskell", "cabal", "hackage", "ghc", "stack"]) {
        s.insert(S::Hackage);
    }
    if idea_contains(idea, &["nix", "nixos", "nixpkgs", "flake"]) {
        s.insert(S::Nixpkgs);
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
        add(&mut s, &[S::CratesIo, S::Go, S::Npm, S::PyPI, S::Homebrew]);
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
        add(&mut s, &[S::VsCodeMarketplace, S::Npm, S::JetBrains]);
    }
    if idea_contains(
        idea,
        &[
            "jetbrains",
            "intellij",
            "pycharm",
            "webstorm",
            "goland",
            "rider",
            "clion",
            "datagrip",
            "phpstorm",
            "rubymine",
            "android studio",
        ],
    ) {
        s.insert(S::JetBrains);
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
        S::Packagist => Box::new(packagist::Packagist::new(client)),
        S::Hex => Box::new(hex::Hex::new(client)),
        S::ArtifactHub => Box::new(artifacthub::ArtifactHub::new(client)),
        S::Aur => Box::new(aur::Aur::new(client)),
        S::Hackage => Box::new(hackage::Hackage::new(client)),
        S::Nixpkgs => Box::new(nixpkgs::Nixpkgs::new(client)),
        S::JetBrains => Box::new(jetbrains::JetBrains::new(client)),
    }
}

/// Pick sources based on what the query is about.
fn sources_for(query: &Query) -> Result<Vec<Box<dyn SourceAdapter>>> {
    let client = http_client()?;
    let ids = detect_sources(&query.idea);
    Ok(ids
        .into_iter()
        .map(|id| build_source(id, client.clone()))
        .collect())
}

/// The outcome of a fan-out: deduped matches, the sources that responded, and
/// the selected sources that failed (so reduced coverage can be surfaced).
pub struct SearchOutcome {
    pub matches: Vec<Match>,
    pub reached: Vec<crate::model::Source>,
    pub failed: Vec<crate::model::Source>,
}

/// Fan out to selected sources concurrently, dropping the ones that fail.
///
/// Returns an error only if the shared HTTP client cannot be built; individual
/// source failures are non-fatal and surfaced via [`SearchOutcome::failed`].
pub async fn search_all(query: &Query) -> Result<SearchOutcome> {
    Ok(search_sources(&sources_for(query)?, query).await)
}

/// Like [`search_all`], but with explicit control over which sources to use.
///
/// When `include` is `Some`, only those sources are searched (overriding the
/// auto-detection heuristic). When `None`, auto-detection runs as usual.
/// Sources in `exclude` are removed from the final set either way.
pub async fn search_filtered(
    query: &Query,
    include: Option<&std::collections::HashSet<crate::model::Source>>,
    exclude: &std::collections::HashSet<crate::model::Source>,
) -> Result<SearchOutcome> {
    let client = http_client()?;
    let mut ids = match include {
        Some(set) => set.clone(),
        None => detect_sources(&query.idea),
    };
    for ex in exclude {
        ids.remove(ex);
    }
    let sources: Vec<Box<dyn SourceAdapter>> = ids
        .into_iter()
        .map(|id| build_source(id, client.clone()))
        .collect();
    Ok(search_sources(&sources, query).await)
}

/// Whether a failed source search is worth a second attempt. Transient failures
/// (network blips, HTML parse drift) are; a persistently unavailable search
/// surface ([`crate::Error::Unavailable`]) is not — a retry would hit the same
/// wall and only add latency.
fn is_retryable(e: &crate::Error) -> bool {
    !matches!(
        e,
        crate::Error::Unavailable(_) | crate::Error::HttpClient(_)
    )
}

/// Per-source wall-clock timeout. Generous enough for the HTTP timeout (10 s)
/// plus the 800 ms retry delay plus a second attempt, but prevents a single slow
/// source from holding up the entire fan-out indefinitely.
const SOURCE_TIMEOUT: Duration = Duration::from_secs(15);

/// Run `query` against `sources` concurrently, skipping any that error, and
/// dedup the combined results. Returns the deduped matches, which sources
/// responded successfully, and which were attempted but failed. Exposed for
/// testing the fan-out in isolation.
pub async fn search_sources(sources: &[Box<dyn SourceAdapter>], query: &Query) -> SearchOutcome {
    let results = join_all(sources.iter().map(|s| {
        let id = s.id();
        async move {
            let outcome = tokio::time::timeout(SOURCE_TIMEOUT, async {
                let first = s.search(query).await;
                match &first {
                    Ok(_) => return (id, first),
                    Err(e) if !is_retryable(e) => return (id, first),
                    Err(_) => {}
                }
                tokio::time::sleep(Duration::from_millis(800)).await;
                (id, s.search(query).await)
            })
            .await;
            match outcome {
                Ok(r) => r,
                Err(_) => (id, Err(crate::Error::Parse(format!("{id} timed out")))),
            }
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
/// order, but filling that first occurrence's gaps from the duplicates behind
/// it. URL is a match's canonical identity across sources.
///
/// Empty/whitespace URLs are never used as a dedup key -- they would collapse
/// all homepage-less entries (e.g. Homebrew formulae with no homepage) into one
/// slot, silently dropping every subsequent one. Those fall through to a
/// (name, source) key instead.
///
/// The enrichment matters because sources know different things about the same
/// artifact: GitHub reports a repo's `pushed_at` and star count, while the
/// Homebrew formula whose homepage *is* that repo reports neither (35% of
/// formulae have a homepage byte-identical to a GitHub repo URL, so this is a
/// routine collision, not a corner case). Dropping the loser wholesale threw
/// that information away -- and since the fan-out iterates a [`HashSet`], which
/// copy arrives first is randomised per process, so the very same query showed
/// a last-updated date on one run and none on the next.
pub fn dedup(matches: Vec<Match>) -> Vec<Match> {
    let mut by_url: HashMap<String, usize> = HashMap::new();
    let mut by_name_source: HashMap<(String, crate::model::Source), usize> = HashMap::new();
    let mut kept: Vec<Match> = Vec::new();

    for m in matches {
        let keyed_by_url = !m.url.trim().is_empty();
        let existing = if keyed_by_url {
            by_url.get(&m.url).copied()
        } else {
            by_name_source.get(&(m.name.clone(), m.source)).copied()
        };

        match existing {
            Some(i) => enrich(&mut kept[i], m),
            None => {
                if keyed_by_url {
                    by_url.insert(m.url.clone(), kept.len());
                } else {
                    by_name_source.insert((m.name.clone(), m.source), kept.len());
                }
                kept.push(m);
            }
        }
    }

    kept
}

/// Fill in what the kept match doesn't know from a later duplicate of the same
/// artifact.
///
/// Only ever fills gaps. A value the kept copy already carries is never
/// overwritten, so which source "owns" a shared URL -- its name, description and
/// the rest -- stays exactly what it was before: the first one to arrive.
fn enrich(kept: &mut Match, dup: Match) {
    if kept.last_updated.is_none() {
        kept.last_updated = dup.last_updated;
    }
    if kept.popularity.is_none() {
        kept.popularity = dup.popularity;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kw(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn q(idea: &str, words: &[&str]) -> Query {
        Query {
            idea: idea.to_string(),
            keywords: kw(words),
        }
    }

    #[test]
    fn narrowed_picks_longest_but_preserves_original_order() {
        // Lengths: ide=3, code=4, spell=5, syntax=6, mistakes=8. The 3
        // longest are mistakes/syntax/spell, but the result must come back
        // in their original (index) order, not length order.
        let keywords = kw(&["ide", "code", "spell", "syntax", "mistakes"]);
        assert_eq!(narrowed(&keywords, 3), "spell syntax mistakes");
        assert_eq!(narrowed(&keywords, 2), "syntax mistakes");
    }

    #[test]
    fn narrowed_is_a_no_op_when_n_covers_all_keywords() {
        let keywords = kw(&["async", "runtime"]);
        assert_eq!(narrowed(&keywords, 3), "async runtime");
        assert_eq!(narrowed(&keywords, 2), "async runtime");
    }

    #[test]
    fn narrowed_of_empty_keywords_is_empty() {
        assert_eq!(narrowed(&[], 3), "");
    }

    #[test]
    fn candidates_go_from_the_full_set_down_to_min_terms() {
        let query = q("x", &["ide", "code", "spell", "syntax", "mistakes"]);
        assert_eq!(
            narrowing_candidates(&query, 2),
            vec![
                "ide code spell syntax mistakes",
                "spell syntax mistakes",
                "syntax mistakes",
            ]
        );
        // Maven only recovers at a single term, so it asks for one more step.
        assert_eq!(
            narrowing_candidates(&query, 1).last().unwrap(),
            "mistakes",
            "min_terms = 1 must end on the single longest keyword"
        );
    }

    #[test]
    fn candidates_drop_narrowings_that_repeat_an_earlier_query() {
        // With <= 3 keywords the "3 longest" step is the full set again.
        let query = q("x", &["async", "runtime"]);
        assert_eq!(narrowing_candidates(&query, 2), vec!["async runtime"]);
    }

    #[test]
    fn candidates_fall_back_to_the_idea_when_there_are_no_keywords() {
        let query = q("kill the process on a port", &[]);
        assert_eq!(
            narrowing_candidates(&query, 2),
            vec!["kill the process on a port"],
            "an empty search param would be sent otherwise"
        );
    }

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
            "a php composer package for laravel",
            "an elixir phoenix library for caching",
            "a helm chart to deploy a kubernetes operator",
            "a pacman helper for installing arch linux aur packages",
            "a haskell cabal library for parsing",
            "a nix flake for building containers",
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
            S::Packagist,
            S::Hex,
            S::ArtifactHub,
            S::Aur,
            S::Hackage,
            S::Nixpkgs,
            S::JetBrains,
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
    fn jetbrains_ide_names_select_jetbrains_even_without_ide_keyword() {
        // Guards the dedicated IDE-name branch specifically: these ideas name
        // a JetBrains product but avoid "ide"/"plugin"/"editor", which would
        // otherwise select JetBrains via the pre-existing vscode/ide branch
        // instead and mask this branch being deleted.
        for idea in [
            "a pycharm helper for databases",
            "a rider tool for unit testing",
            "a clion helper for cmake projects",
        ] {
            let s = detect_sources(idea);
            assert!(
                s.contains(&S::JetBrains),
                "JetBrains missing for {idea:?}: {s:?}"
            );
        }
    }

    #[test]
    fn no_signal_falls_back_to_broad_sweep() {
        let s = detect_sources("asdf qwer zxcv hjkl");
        assert!(s.contains(&S::Npm));
        assert!(s.contains(&S::PyPI));
        assert!(s.contains(&S::CratesIo));
    }

    #[test]
    fn from_str_parses_known_sources() {
        assert_eq!("npm".parse::<crate::model::Source>().unwrap(), S::Npm);
        assert_eq!(
            "crates-io".parse::<crate::model::Source>().unwrap(),
            S::CratesIo
        );
        assert_eq!(
            "docker".parse::<crate::model::Source>().unwrap(),
            S::DockerHub
        );
        assert_eq!(
            "vscode".parse::<crate::model::Source>().unwrap(),
            S::VsCodeMarketplace
        );
        assert_eq!("nix".parse::<crate::model::Source>().unwrap(), S::Nixpkgs);
        assert_eq!(
            "jetbrains".parse::<crate::model::Source>().unwrap(),
            S::JetBrains
        );
        assert_eq!(
            "intellij".parse::<crate::model::Source>().unwrap(),
            S::JetBrains
        );
        assert!(
            "unknown".parse::<crate::model::Source>().is_err(),
            "an unrecognised source name must return Err"
        );
    }

    #[test]
    fn http_client_builds() {
        // The client builder is fallible (no longer `.expect()`s); on a normal
        // host it builds cleanly.
        assert!(http_client().is_ok());
    }

    #[test]
    fn sources_for_builds_selected_adapters() {
        let q = Query {
            idea: "a rust crate for parsing".to_string(),
            keywords: vec![],
        };
        let sources = sources_for(&q).expect("client should build");
        assert!(!sources.is_empty());
    }
}
