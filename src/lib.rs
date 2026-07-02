//! `patent` — a prior-art search for your code ideas.
//!
//! Takes a plain-English dev-tool idea and searches the open-source ecosystem —
//! crates.io, npm, PyPI, GitHub, Go, Maven, NuGet, RubyGems, Docker Hub,
//! Homebrew, Packagist, Hex, Artifact Hub, AUR, the VS Code Marketplace, and
//! Hacker News — for prior art, then gives an honest,
//! scoped verdict on whether it's already been built. The exact set searched is
//! chosen per query; whichever sources actually responded are always surfaced.
//!
//! **Integrity principle:** this tool can prove something *exists*, but never
//! that it *doesn't* — it only searched some sources. All output is scoped to
//! "what was found in the sources checked."
//!
//! # Install
//!
//! ```bash
//! cargo install patent
//! ```
//!
//! # Usage
//!
//! ```bash
//! patent "interactive cli to kill whatever's on a port"   # interactive TUI
//! patent "react component for infinite scroll" --json      # structured output
//! patent "kubernetes log viewer" --fast                    # skip the LLM verdict
//! patent "vector database" --api-base https://api.openai.com/v1 --model gpt-4o-mini
//! ```
//!
//! # Using the library
//!
//! `patent` is primarily the engine behind the CLI of the same name, but the
//! core is reusable: [`sources::search_all`] fans out to the registries,
//! [`rank`] (or the async-safe [`rank::rank_async`]) orders matches by
//! semantic similarity, and [`verdict::assess`]
//! turns them into an integrity-scoped [`Verdict`] via any [`Llm`] backend
//! (local Ollama or an OpenAI-compatible API).

pub mod llm;
pub mod model;
pub mod ollama;
pub mod openai;
pub mod rank;
pub mod sources;
#[doc(hidden)]
pub mod tui;
pub mod verdict;

pub use llm::Llm;
pub use model::{Match, Query, Saturation, Source, Verdict};

/// Library-level error type. The binary maps these to `anyhow` with context.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("http request failed: {0}")]
    Http(#[from] reqwest::Error),

    /// The shared HTTP client could not be constructed (e.g. the TLS backend
    /// failed to initialize). Surfaced instead of panicking so library
    /// consumers can handle it. Distinct from [`Error::Http`] (a request that
    /// was sent and failed) — this is a failure to build the client at all.
    #[error("failed to build HTTP client: {0}")]
    HttpClient(#[source] reqwest::Error),

    #[error("failed to parse response: {0}")]
    Parse(String),

    /// LLM endpoint could not be reached. The message carries the address and a hint.
    #[error("{0}")]
    LlmUnreachable(String),

    /// LLM endpoint was reached but rejected the request (unknown model, bad key,
    /// server error). The message carries the reason and a hint.
    #[error("{0}")]
    LlmRejected(String),

    #[error("embedding failed: {0}")]
    Embedding(String),
}

/// Crate result alias.
pub type Result<T> = std::result::Result<T, Error>;
