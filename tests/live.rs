//! Live smoke tests — the ONLY tests that hit real registry APIs.
//!
//! Every test here is `#[ignore]`d, so `cargo test` stays fully hermetic and
//! offline: the default suite exercises the adapters against `wiremock` canned
//! responses (see `tests/sources.rs`). These complement that with the one thing
//! mocks can't catch — an upstream API silently changing its response shape,
//! which would leave the mocked tests green while the real tool returns nothing.
//!
//! Run them explicitly (they need network, and are subject to upstream rate
//! limits and outages, so they are not part of PR CI):
//!
//! ```bash
//! cargo test --test live -- --ignored --nocapture
//! ```
//!
//! A nightly CI job (`.github/workflows/live.yml`) runs them on a schedule so
//! drift is surfaced as a failed run rather than a silent zero-results bug.
//!
//! Each query uses a deliberately popular, stable keyword for its ecosystem, so
//! an empty result set means the adapter can no longer parse the live response —
//! not that the query happened to match nothing.

use patent::model::{Match, Query, Source as SourceId};
use patent::sources::artifacthub::ArtifactHub;
use patent::sources::aur::Aur;
use patent::sources::crates_io::CratesIo;
use patent::sources::docker_hub::DockerHub;
use patent::sources::github::GitHub;
use patent::sources::go::GoPkgDev;
use patent::sources::hackage::Hackage;
use patent::sources::hacker_news::HackerNews;
use patent::sources::hex::Hex;
use patent::sources::homebrew::Homebrew;
use patent::sources::maven::Maven;
use patent::sources::npm::Npm;
use patent::sources::nuget::NuGet;
use patent::sources::packagist::Packagist;
use patent::sources::pypi::PyPI;
use patent::sources::rubygems::RubyGems;
use patent::sources::vscode::VsCodeMarketplace;
use patent::sources::SourceAdapter;

/// A real HTTP client, mirroring the one the binary uses (registries such as
/// crates.io reject requests without a descriptive user-agent).
fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!(
            "patent/",
            env!("CARGO_PKG_VERSION"),
            " (prior-art search; https://github.com/r14dd/patent)"
        ))
        .build()
        .unwrap()
}

/// Build a query. `idea` matters for the sources that search on it (npm, Hacker
/// News); `keywords` matters for everyone else. Both are set so a single query
/// is valid regardless of which field an adapter reads.
fn query(idea: &str, keywords: &[&str]) -> Query {
    Query {
        idea: idea.to_string(),
        keywords: keywords.iter().map(|k| k.to_string()).collect(),
    }
}

/// Shared assertions: a live search for a popular term must return well-formed
/// matches attributed to the expected source. An empty set is the drift signal.
fn assert_live(matches: &[Match], expected: SourceId) {
    assert!(
        !matches.is_empty(),
        "{expected} returned zero matches for a popular query — the live API \
         shape has likely drifted and the adapter can no longer parse it"
    );
    for m in matches {
        assert!(
            !m.name.trim().is_empty(),
            "{expected}: a match has an empty name: {m:?}"
        );
        assert!(
            !m.url.trim().is_empty(),
            "{expected}: match {:?} has an empty url",
            m.name
        );
        assert_eq!(
            m.source, expected,
            "{expected}: a match is attributed to the wrong source"
        );
    }
    eprintln!("✓ {expected}: {} matches", matches.len());
}

macro_rules! live {
    ($name:ident, $adapter:expr, $source:expr, $idea:expr, $keywords:expr) => {
        #[tokio::test]
        #[ignore = "hits the live network; run with --ignored"]
        async fn $name() {
            let adapter = $adapter;
            let matches = adapter
                .search(&query($idea, $keywords))
                .await
                .unwrap_or_else(|e| panic!("{} live search errored: {e}", $source));
            assert_live(&matches, $source);
        }
    };
}

live!(
    live_crates_io,
    CratesIo::new(client()),
    SourceId::CratesIo,
    "a fast async runtime for rust",
    &["async", "runtime"]
);

live!(
    live_github,
    GitHub::new(client()),
    SourceId::GitHub,
    "a kubernetes command line tool",
    &["kubernetes", "cli"]
);

live!(
    live_npm,
    Npm::new(client()),
    SourceId::Npm,
    "react component library",
    &["react"]
);

// KNOWN, ACCEPTED DEGRADATION — not a pending fix. PyPI has retired every keyless
// search path: the XML-RPC `search` endpoint is gone, and the web search page now
// sits behind a Fastly "Client Challenge" bot wall that serves a JS stub to any
// non-browser client (verified: identical challenge for a descriptive UA and a
// browser UA). The only remaining programmatic search needs an API key, which was
// deliberately rejected as against this tool's keyless, no-friction ethos. So PyPI
// genuinely cannot return results and this smoke test cannot pass; the nightly
// skips it with `--skip live_pypi`. As of 0.7.1 the adapter fails *honestly* —
// `Error::Unavailable` with accurate wording — so the binary surfaces PyPI as
// "not reached", never as an empty result. Kept (not deleted) so that if a keyless
// backend ever appears, restoring it is just: swap it in, then drop the `--skip`.
#[tokio::test]
#[ignore = "live network; also an ACCEPTED DEGRADATION — PyPI is bot-walled with no keyless search, see comment"]
async fn live_pypi() {
    let adapter = PyPI::new(client());
    let matches = adapter
        .search(&query("http requests library for python", &["requests"]))
        .await
        .unwrap_or_else(|e| panic!("PyPI live search errored: {e}"));
    assert_live(&matches, SourceId::PyPI);
}

live!(
    live_hacker_news,
    HackerNews::new(client()),
    SourceId::HackerNews,
    "database",
    &["database"]
);

live!(
    live_go,
    GoPkgDev::new(client()),
    SourceId::Go,
    "web framework for go",
    &["gin"]
);

live!(
    live_maven,
    Maven::new(client()),
    SourceId::Maven,
    "json serialization library",
    &["jackson"]
);

live!(
    live_rubygems,
    RubyGems::new(client()),
    SourceId::RubyGems,
    "a ruby web framework",
    &["rails"]
);

live!(
    live_docker_hub,
    DockerHub::new(client()),
    SourceId::DockerHub,
    "a web server container image",
    &["nginx"]
);

live!(
    live_vscode,
    VsCodeMarketplace::new(client()),
    SourceId::VsCodeMarketplace,
    "python language support",
    &["python"]
);

live!(
    live_nuget,
    NuGet::new(client()),
    SourceId::NuGet,
    "a json library",
    &["json"]
);

live!(
    live_homebrew,
    Homebrew::new(client()),
    SourceId::Homebrew,
    "download files from the web",
    &["wget"]
);

live!(
    live_packagist,
    Packagist::new(client()),
    SourceId::Packagist,
    "a php web framework",
    &["laravel"]
);

live!(
    live_hex,
    Hex::new(client()),
    SourceId::Hex,
    "an elixir web framework",
    &["phoenix"]
);

live!(
    live_artifacthub,
    ArtifactHub::new(client()),
    SourceId::ArtifactHub,
    "prometheus monitoring helm chart",
    &["prometheus"]
);

live!(
    live_aur,
    Aur::new(client()),
    SourceId::Aur,
    "an aur helper",
    &["yay"]
);

live!(
    live_hackage,
    Hackage::new(client()),
    SourceId::Hackage,
    "a haskell json parsing library",
    &["json"]
);
