//! Integration tests for the source adapters.
//!
//! Each source is exercised against a `wiremock` server serving canned registry
//! responses — no live network in CI. Filled in alongside M1/M2.

// M1: crates.io parsing via wiremock.
// M2: each remaining source + dedup.
use patent::model::{Match, Query, Source as SourceId};
use patent::sources::artifacthub::ArtifactHub;
use patent::sources::aur::Aur;
use patent::sources::crates_io::CratesIo;
use patent::sources::docker_hub::DockerHub;
use patent::sources::github::GitHub;
use patent::sources::go::GoPkgDev;
use patent::sources::hacker_news::HackerNews;
use patent::sources::hex::Hex;
use patent::sources::homebrew::Homebrew;
use patent::sources::jetbrains::JetBrains;
use patent::sources::maven::Maven;
use patent::sources::nixpkgs::Nixpkgs;
use patent::sources::npm::Npm;
use patent::sources::nuget::NuGet;
use patent::sources::packagist::Packagist;
use patent::sources::pypi::PyPI;
use patent::sources::rubygems::RubyGems;
use patent::sources::vscode::VsCodeMarketplace;
use patent::sources::{dedup, search_sources, SearchOutcome, SourceAdapter};
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use wiremock::matchers::{
    header_exists, method, path, path_regex, query_param, query_param_is_missing,
};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn test_client() -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(concat!(
            "patent/",
            env!("CARGO_PKG_VERSION"),
            " (prior-art search; https://github.com/r14dd/patent)"
        ))
        .build()
        .unwrap()
}

/// A query whose keywords join to the string we assert on the wire.
fn query() -> Query {
    Query {
        idea: "a fast async runtime for rust".to_string(),
        keywords: vec!["async".to_string(), "runtime".to_string()],
    }
}

/// Build a `CratesIo` whose requests are aimed at the mock server.
fn source_for(server: &MockServer) -> CratesIo {
    CratesIo::with_base_url(test_client(), server.uri())
}

/// A canonical crates.io search payload with two hits.
fn two_crate_body() -> serde_json::Value {
    json!({
        "crates": [
            {
                "name": "tokio",
                "updated_at": "2026-05-15T06:13:41.215606Z",
                "description": "An event-driven, non-blocking I/O platform.",
                "downloads": 950_000_000_u64,
                "max_version": "1.40.0",
                "repository": "https://github.com/tokio-rs/tokio"
            },
            {
                "name": "async-std",
                "description": "Async version of the Rust standard library.",
                "downloads": 45_000_000_u64,
                "max_version": "1.13.0",
                "repository": "https://github.com/async-rs/async-std"
            }
        ],
        "meta": { "total": 2 }
    })
}

#[tokio::test]
async fn crates_io_id_is_crates_io() {
    let src = CratesIo::with_base_url(reqwest::Client::new(), "https://crates.io".to_string());
    assert_eq!(src.id(), SourceId::CratesIo);
}

#[tokio::test]
async fn crates_io_maps_results_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_crate_body()))
        .mount(&server)
        .await;

    let matches = source_for(&server).search(&query()).await.unwrap();

    assert_eq!(matches.len(), 2);

    let tokio = &matches[0];
    assert_eq!(tokio.name, "tokio");
    assert_eq!(tokio.source, SourceId::CratesIo);
    assert_eq!(
        tokio.description,
        "An event-driven, non-blocking I/O platform."
    );
    assert_eq!(tokio.popularity, Some(950_000_000));
    // similarity is filled in later by rank.rs; sources leave it at 0.0.
    assert_eq!(tokio.similarity, 0.0);

    let async_std = &matches[1];
    assert_eq!(async_std.name, "async-std");
    assert_eq!(async_std.popularity, Some(45_000_000));
}

#[tokio::test]
async fn crates_io_links_use_the_configured_base_url() {
    // The result `url` field is built from `self.base_url`, so a request
    // served by a mock server (or a crates.io mirror) should surface links
    // against the same host we queried, not the hard-coded production host.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_crate_body()))
        .mount(&server)
        .await;

    let matches = source_for(&server).search(&query()).await.unwrap();

    assert_eq!(matches[0].url, format!("{}/crates/tokio", server.uri()));
    assert_eq!(matches[1].url, format!("{}/crates/async-std", server.uri()));
}

#[tokio::test]
async fn crates_io_sends_joined_keywords_as_query_and_a_user_agent() {
    let server = MockServer::start().await;
    // The mock only matches if `q` is the space-joined keywords AND a
    // User-Agent header is present (crates.io rejects requests without one).
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .and(query_param("q", "async runtime"))
        .and(header_exists("user-agent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_crate_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = source_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
    // `expect(1)` is verified on server drop: the request matched our assertions.
}

#[tokio::test]
async fn crates_io_user_agent_contains_correct_github_handle() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_crate_body()))
        .mount(&server)
        .await;

    source_for(&server).search(&query()).await.unwrap();

    let requests = server.received_requests().await.unwrap();
    let ua = requests[0]
        .headers
        .get("user-agent")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        ua.contains("r14dd/patent"),
        "expected 'r14dd/patent' in User-Agent, got: {ua}"
    );
}

#[tokio::test]
async fn crates_io_null_description_becomes_empty_string() {
    let server = MockServer::start().await;
    let body = json!({
        "crates": [
            { "name": "mystery-crate", "description": null, "downloads": 7 }
        ],
        "meta": { "total": 1 }
    });
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = source_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].description, "");
}

#[tokio::test]
async fn crates_io_missing_downloads_becomes_none() {
    let server = MockServer::start().await;
    let body = json!({
        "crates": [
            { "name": "obscure", "description": "no download count here" }
        ],
        "meta": { "total": 1 }
    });
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = source_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].popularity, None);
}

#[tokio::test]
async fn crates_io_empty_results_is_ok_not_error() {
    let server = MockServer::start().await;
    let body = json!({ "crates": [], "meta": { "total": 0 } });
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = source_for(&server).search(&query()).await.unwrap();
    assert!(matches.is_empty());
}

#[tokio::test]
async fn crates_io_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let result = source_for(&server).search(&query()).await;
    assert!(result.is_err(), "a 5xx response must surface as an error");
}

#[tokio::test]
async fn crates_io_malformed_body_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json at all"))
        .mount(&server)
        .await;

    let result = source_for(&server).search(&query()).await;
    assert!(
        result.is_err(),
        "an unparseable body must surface as an error"
    );
}

// ---------------------------------------------------------------------------
// GitHub
// ---------------------------------------------------------------------------

fn github_for(server: &MockServer) -> GitHub {
    GitHub::with_base_url(test_client(), server.uri())
}

fn github_body() -> serde_json::Value {
    json!({
        "total_count": 2,
        "items": [
            {
                "full_name": "tokio-rs/tokio",
                "pushed_at": "2026-07-31T18:49:43Z",
                "description": "An async runtime for Rust.",
                "html_url": "https://github.com/tokio-rs/tokio",
                "stargazers_count": 27000
            },
            {
                "full_name": "async-rs/async-std",
                "description": "Async std library.",
                "html_url": "https://github.com/async-rs/async-std",
                "stargazers_count": 4000
            }
        ]
    })
}

#[tokio::test]
async fn github_id_is_github() {
    let src = GitHub::with_base_url(reqwest::Client::new(), "https://api.github.com".to_string());
    assert_eq!(src.id(), SourceId::GitHub);
}

#[tokio::test]
async fn github_maps_repositories_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(github_body()))
        .mount(&server)
        .await;

    let matches = github_for(&server).search(&query()).await.unwrap();

    assert_eq!(matches.len(), 2);
    let first = &matches[0];
    assert_eq!(first.name, "tokio-rs/tokio");
    assert_eq!(first.source, SourceId::GitHub);
    assert_eq!(first.url, "https://github.com/tokio-rs/tokio");
    assert_eq!(first.description, "An async runtime for Rust.");
    assert_eq!(first.popularity, Some(27000));
    assert_eq!(first.similarity, 0.0);
}

#[tokio::test]
async fn github_sends_query_and_user_agent() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .and(query_param("q", "async runtime in:description,readme"))
        // #31: no `sort` param (GitHub defaults to best-match relevance, not
        // raw stars) and a wider page so low-star on-topic repos can surface.
        .and(query_param_is_missing("sort"))
        .and(query_param("per_page", "50"))
        .and(header_exists("user-agent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(github_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = github_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
}

#[tokio::test]
async fn github_null_description_is_filtered_out() {
    let server = MockServer::start().await;
    let body = json!({
        "total_count": 1,
        "items": [
            { "full_name": "x/y", "description": null,
              "html_url": "https://github.com/x/y", "stargazers_count": 1 }
        ]
    });
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = github_for(&server).search(&query()).await.unwrap();
    assert!(matches.is_empty());
}

#[tokio::test]
async fn github_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "total_count": 0, "items": [] });
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    assert!(github_for(&server)
        .search(&query())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn github_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    assert!(github_for(&server).search(&query()).await.is_err());
}

#[tokio::test]
async fn github_malformed_body_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;

    let result = github_for(&server).search(&query()).await;
    assert!(
        result.is_err(),
        "an unparseable body must surface as an error"
    );
}

#[tokio::test]
async fn github_401_with_token_returns_auth_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(401))
        .mount(&server)
        .await;

    let src = GitHub::with_base_url_and_token(
        reqwest::Client::new(),
        server.uri(),
        "bad-token".to_string(),
    );
    let err = src.search(&query()).await.unwrap_err();
    let msg = err.to_string().to_lowercase();
    assert!(
        msg.contains("401") || msg.contains("token") || msg.contains("invalid"),
        "expected auth error, got: {err}"
    );
}

#[tokio::test]
async fn github_403_with_token_returns_rate_limit_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(403).append_header("X-RateLimit-Remaining", "0"))
        .mount(&server)
        .await;

    let src = GitHub::with_base_url_and_token(
        reqwest::Client::new(),
        server.uri(),
        "valid-token".to_string(),
    );
    let err = src.search(&query()).await.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("rate limit"),
        "expected rate limit error, got: {err}"
    );
}

#[tokio::test]
async fn github_403_without_token_returns_rate_limit_hint() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/repositories"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let err = github_for(&server).search(&query()).await.unwrap_err();
    assert!(
        err.to_string().contains("GITHUB_TOKEN"),
        "expected GITHUB_TOKEN hint, got: {err}"
    );
}

// ---------------------------------------------------------------------------
// npm
// ---------------------------------------------------------------------------

fn npm_for(server: &MockServer) -> Npm {
    Npm::with_base_url(reqwest::Client::new(), server.uri())
}

fn npm_body() -> serde_json::Value {
    json!({
        "total": 2,
        "objects": [
            { "package": { "name": "express", "description": "Fast web framework.", "date": "2026-07-21T15:41:28.716Z" } },
            { "package": { "name": "koa", "description": "Next-gen web framework." } }
        ]
    })
}

#[tokio::test]
async fn npm_id_is_npm() {
    let src = Npm::with_base_url(
        reqwest::Client::new(),
        "https://registry.npmjs.org".to_string(),
    );
    assert_eq!(src.id(), SourceId::Npm);
}

#[tokio::test]
async fn npm_maps_packages_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/-/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(npm_body()))
        .mount(&server)
        .await;

    let matches = npm_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
    let first = &matches[0];
    assert_eq!(first.name, "express");
    assert_eq!(first.source, SourceId::Npm);
    assert_eq!(first.url, "https://www.npmjs.com/package/express");
    assert_eq!(first.description, "Fast web framework.");
    // npm search exposes no integer download count.
    assert_eq!(first.popularity, None);
    assert_eq!(first.similarity, 0.0);
}

#[tokio::test]
async fn npm_sends_text_query_param() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/-/v1/search"))
        .and(query_param("text", "a fast async runtime for rust"))
        .respond_with(ResponseTemplate::new(200).set_body_json(npm_body()))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(npm_for(&server).search(&query()).await.unwrap().len(), 2);
}

#[tokio::test]
async fn npm_missing_description_becomes_empty() {
    let server = MockServer::start().await;
    let body = json!({ "total": 1, "objects": [ { "package": { "name": "bare" } } ] });
    Mock::given(method("GET"))
        .and(path("/-/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = npm_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches[0].description, "");
}

#[tokio::test]
async fn npm_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "total": 0, "objects": [] });
    Mock::given(method("GET"))
        .and(path("/-/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    assert!(npm_for(&server).search(&query()).await.unwrap().is_empty());
}

#[tokio::test]
async fn npm_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/-/v1/search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(npm_for(&server).search(&query()).await.is_err());
}

#[tokio::test]
async fn npm_malformed_body_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/-/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;

    let result = npm_for(&server).search(&query()).await;
    assert!(
        result.is_err(),
        "an unparseable body must surface as an error"
    );
}

// ---------------------------------------------------------------------------
// PyPI (HTML scrape)
// ---------------------------------------------------------------------------

fn pypi_for(server: &MockServer) -> PyPI {
    PyPI::with_base_url(reqwest::Client::new(), server.uri())
}

fn pypi_html() -> String {
    r#"<!doctype html><html><body>
      <ul>
        <li>
          <a class="package-snippet" href="/project/requests/">
            <h3 class="package-snippet__title">
              <span class="package-snippet__name">requests</span>
              <span class="package-snippet__version">2.31.0</span>
            </h3>
            <p class="package-snippet__description">Python HTTP for Humans.</p>
          </a>
        </li>
        <li>
          <a class="package-snippet" href="/project/httpx/">
            <h3 class="package-snippet__title">
              <span class="package-snippet__name">httpx</span>
              <span class="package-snippet__version">0.27.0</span>
            </h3>
            <p class="package-snippet__description">A next-gen HTTP client.</p>
          </a>
        </li>
      </ul>
    </body></html>"#
        .to_string()
}

#[tokio::test]
async fn pypi_id_is_pypi() {
    let src = PyPI::with_base_url(reqwest::Client::new(), "https://pypi.org".to_string());
    assert_eq!(src.id(), SourceId::PyPI);
}

#[tokio::test]
async fn pypi_scrapes_packages_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(pypi_html()))
        .mount(&server)
        .await;

    let matches = pypi_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
    let first = &matches[0];
    assert_eq!(first.name, "requests");
    assert_eq!(first.source, SourceId::PyPI);
    // The URL is built from the (mock) base_url + the relative href.
    assert!(first.url.ends_with("/project/requests/"));
    assert_eq!(first.description, "Python HTTP for Humans.");
    assert_eq!(first.popularity, None);
    assert_eq!(first.similarity, 0.0);

    assert_eq!(matches[1].name, "httpx");
    assert!(matches[1].url.ends_with("/project/httpx/"));
}

#[tokio::test]
async fn pypi_sends_q_query_param() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/"))
        .and(query_param("q", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_string(pypi_html()))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(pypi_for(&server).search(&query()).await.unwrap().len(), 2);
}

#[tokio::test]
async fn pypi_no_snippets_is_ok_empty() {
    let server = MockServer::start().await;
    let html = "<!doctype html><html><body><p>No results.</p></body></html>";
    Mock::given(method("GET"))
        .and(path("/search/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(&server)
        .await;

    assert!(pypi_for(&server).search(&query()).await.unwrap().is_empty());
}

#[tokio::test]
async fn pypi_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(pypi_for(&server).search(&query()).await.is_err());
}

#[tokio::test]
async fn pypi_forbidden_is_unavailable() {
    // A hard 403 from the bot wall must surface as Unavailable (non-retryable),
    // not a generic HTTP error, so the reason is accurate and the retry is skipped.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search/"))
        .respond_with(ResponseTemplate::new(403))
        .mount(&server)
        .await;

    let err = pypi_for(&server).search(&query()).await.unwrap_err();
    assert!(
        matches!(err, patent::Error::Unavailable(_)),
        "403 bot wall should be Error::Unavailable, got: {err:?}"
    );
}

#[tokio::test]
async fn pypi_bot_wall_200_challenge_is_unavailable() {
    // The live wall serves a 200 whose body is a JS challenge stub: a non-trivial
    // page (>2000 bytes) with zero package snippets. That must read as Unavailable
    // (the real cause), not the misleading "markup may have changed" parse error.
    let server = MockServer::start().await;
    let stub = format!(
        "<!doctype html><html><head><title>Client Challenge</title></head><body>{}</body></html>",
        "x".repeat(2_500)
    );
    Mock::given(method("GET"))
        .and(path("/search/"))
        .respond_with(ResponseTemplate::new(200).set_body_string(stub))
        .mount(&server)
        .await;

    let err = pypi_for(&server).search(&query()).await.unwrap_err();
    assert!(
        matches!(err, patent::Error::Unavailable(_)),
        "200 challenge stub should be Error::Unavailable, got: {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Hacker News (Algolia)
// ---------------------------------------------------------------------------

fn hn_for(server: &MockServer) -> HackerNews {
    HackerNews::with_base_url(reqwest::Client::new(), server.uri())
}

fn hn_body() -> serde_json::Value {
    json!({
        "hits": [
            {
                "title": "Show HN: Tokio, an async runtime",
                "story_text": "We built an async runtime.",
                "url": "https://tokio.rs",
                "objectID": "12345",
                "points": 320
            },
            {
                "title": "Async Rust explained",
                "story_text": null,
                "url": null,
                "objectID": "67890",
                "points": 88
            }
        ]
    })
}

#[tokio::test]
async fn hn_id_is_hacker_news() {
    let src =
        HackerNews::with_base_url(reqwest::Client::new(), "https://hn.algolia.com".to_string());
    assert_eq!(src.id(), SourceId::HackerNews);
}

#[tokio::test]
async fn hn_maps_hits_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(hn_body()))
        .mount(&server)
        .await;

    let matches = hn_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
    let first = &matches[0];
    assert_eq!(first.name, "Show HN: Tokio, an async runtime");
    assert_eq!(first.source, SourceId::HackerNews);
    // Canonical HN URL is the discussion item, not the (optional) outbound link.
    assert_eq!(first.url, "https://news.ycombinator.com/item?id=12345");
    assert_eq!(first.description, "We built an async runtime.");
    assert_eq!(first.popularity, Some(320));
    assert_eq!(first.similarity, 0.0);
}

#[tokio::test]
async fn hn_null_story_text_falls_back_to_title() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(hn_body()))
        .mount(&server)
        .await;

    let matches = hn_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches[1].description, "Async Rust explained");
    assert_eq!(matches[1].url, "https://news.ycombinator.com/item?id=67890");
    assert_eq!(matches[1].popularity, Some(88));
}

#[tokio::test]
async fn hn_sends_query_param() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .and(query_param("query", "a fast async runtime for rust"))
        .respond_with(ResponseTemplate::new(200).set_body_json(hn_body()))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(hn_for(&server).search(&query()).await.unwrap().len(), 2);
}

#[tokio::test]
async fn hn_empty_hits_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "hits": [] });
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    assert!(hn_for(&server).search(&query()).await.unwrap().is_empty());
}

#[tokio::test]
async fn hn_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(hn_for(&server).search(&query()).await.is_err());
}

#[tokio::test]
async fn hn_malformed_body_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;

    let result = hn_for(&server).search(&query()).await;
    assert!(
        result.is_err(),
        "an unparseable body must surface as an error"
    );
}

#[tokio::test]
async fn hn_strips_html_from_story_text() {
    let server = MockServer::start().await;
    let body = json!({
        "hits": [{
            "title": "Show HN: Something",
            "story_text": "<p>A tool that <b>does</b> things.</p>",
            "objectID": "99999",
            "points": 10
        }]
    });
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = hn_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches[0].description, "A tool that does things.");
}

#[tokio::test]
async fn hn_bare_ampersand_does_not_eat_remaining_text() {
    // A bare '&' with no closing ';' must be emitted as a literal and must NOT
    // consume the text that follows it.
    let server = MockServer::start().await;
    let body = json!({
        "hits": [{
            "title": "Show HN: Something",
            "story_text": "fast tool for C & C++",
            "objectID": "11111",
            "points": 5
        }]
    });
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = hn_for(&server).search(&query()).await.unwrap();
    assert_eq!(
        matches[0].description, "fast tool for C & C++",
        "text after bare '&' must not be eaten"
    );
}

// ---------------------------------------------------------------------------
// Fan-out + dedup
// ---------------------------------------------------------------------------

/// A source that always fails — stands in for an unreachable registry.
struct FailingSource;

#[async_trait::async_trait]
impl SourceAdapter for FailingSource {
    fn id(&self) -> SourceId {
        SourceId::PyPI
    }
    async fn search(&self, _query: &Query) -> patent::Result<Vec<Match>> {
        Err(patent::Error::Parse("simulated failure".to_string()))
    }
}

fn sample_match(name: &str, url: &str) -> Match {
    Match {
        name: name.to_string(),
        source: SourceId::CratesIo,
        url: url.to_string(),
        description: String::new(),
        popularity: None,
        similarity: 0.0,
        last_updated: None,
    }
}

#[test]
fn dedup_removes_same_url_keeping_first_and_order() {
    let input = vec![
        sample_match("a", "https://x/a"),
        sample_match("b", "https://x/b"),
        sample_match("a-dup", "https://x/a"), // same URL as the first
        sample_match("c", "https://x/c"),
    ];
    let out = dedup(input);
    let urls: Vec<&str> = out.iter().map(|m| m.url.as_str()).collect();
    assert_eq!(urls, ["https://x/a", "https://x/b", "https://x/c"]);
    // First occurrence is kept, not the later duplicate.
    assert_eq!(out[0].name, "a");
}

#[test]
fn dedup_keeps_distinct_urls() {
    let input = vec![
        sample_match("a", "https://x/a"),
        sample_match("b", "https://x/b"),
    ];
    assert_eq!(dedup(input).len(), 2);
}

/// The real collision: a Homebrew formula whose homepage *is* a GitHub repo URL,
/// alongside GitHub's own entry for that repo. Homebrew publishes no date and no
/// star count; GitHub publishes both. Fan-out order is randomised (it iterates a
/// `HashSet`), so if the loser were dropped wholesale the same query would show
/// a date on one run and not the next.
#[test]
fn dedup_fills_gaps_in_the_kept_match_from_a_later_duplicate() {
    let url = "https://github.com/skim-rs/skim";
    let brew = Match {
        name: "sk".to_string(),
        source: SourceId::Homebrew,
        url: url.to_string(),
        description: "Fuzzy Finder in rust!".to_string(),
        popularity: None,
        similarity: 0.0,
        last_updated: None,
    };
    let github = Match {
        name: "skim-rs/skim".to_string(),
        source: SourceId::GitHub,
        url: url.to_string(),
        description: "Fuzzy Finder in rust!".to_string(),
        popularity: Some(5_600),
        similarity: 0.0,
        last_updated: Some("2026-08-02T08:39:42Z".to_string()),
    };

    let out = dedup(vec![brew, github]);

    assert_eq!(out.len(), 1);
    // Identity still belongs to the first arrival — enrichment fills gaps, it
    // does not hand the row to the other source.
    assert_eq!(out[0].name, "sk");
    assert_eq!(out[0].source, SourceId::Homebrew);
    // ...but what only the duplicate knew is no longer lost.
    assert_eq!(out[0].last_updated.as_deref(), Some("2026-08-02T08:39:42Z"));
    assert_eq!(out[0].popularity, Some(5_600));
}

#[test]
fn dedup_never_overwrites_what_the_kept_match_already_knows() {
    let mut first = sample_match("a", "https://x/a");
    first.last_updated = Some("2020-01-01T00:00:00Z".to_string());
    first.popularity = Some(1);

    let mut later = sample_match("a-dup", "https://x/a");
    later.last_updated = Some("2026-01-01T00:00:00Z".to_string());
    later.popularity = Some(999);

    let out = dedup(vec![first, later]);

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].last_updated.as_deref(), Some("2020-01-01T00:00:00Z"));
    assert_eq!(out[0].popularity, Some(1));
}

/// Homepage-less entries fall through to a (name, source) key, and that path
/// enriches too.
#[test]
fn dedup_enriches_across_the_name_source_key_as_well() {
    let first = sample_match("lonely", "");
    let mut later = sample_match("lonely", "");
    later.last_updated = Some("2026-03-01T00:00:00Z".to_string());

    let out = dedup(vec![first, later]);

    assert_eq!(out.len(), 1);
    assert_eq!(out[0].last_updated.as_deref(), Some("2026-03-01T00:00:00Z"));
}

#[tokio::test]
async fn search_sources_skips_failing_sources() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_crate_body()))
        .mount(&server)
        .await;

    let sources: Vec<Box<dyn SourceAdapter>> = vec![
        Box::new(FailingSource),
        Box::new(CratesIo::with_base_url(
            reqwest::Client::new(),
            server.uri(),
        )),
    ];

    // The failing source is skipped, never fatal: we still get the crates,
    // and the failed source is reported so reduced coverage is visible.
    let SearchOutcome {
        matches,
        reached,
        failed,
    } = search_sources(&sources, &query()).await;
    assert_eq!(matches.len(), 2);
    assert!(matches.iter().all(|m| m.source == SourceId::CratesIo));
    assert_eq!(reached, vec![SourceId::CratesIo]);
    assert_eq!(failed, vec![SourceId::PyPI]);
}

#[tokio::test]
async fn search_sources_dedups_across_sources() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_crate_body()))
        .mount(&server)
        .await;

    // Two identical crates.io sources -> overlapping URLs -> deduped to 2.
    let sources: Vec<Box<dyn SourceAdapter>> = vec![
        Box::new(CratesIo::with_base_url(
            reqwest::Client::new(),
            server.uri(),
        )),
        Box::new(CratesIo::with_base_url(
            reqwest::Client::new(),
            server.uri(),
        )),
    ];

    let SearchOutcome {
        matches, reached, ..
    } = search_sources(&sources, &query()).await;
    assert_eq!(matches.len(), 2);
    assert_eq!(reached.len(), 2);
}

#[tokio::test]
async fn search_sources_empty_when_all_fail() {
    let sources: Vec<Box<dyn SourceAdapter>> =
        vec![Box::new(FailingSource), Box::new(FailingSource)];
    let SearchOutcome {
        matches,
        reached,
        failed,
    } = search_sources(&sources, &query()).await;
    assert!(matches.is_empty());
    assert!(reached.is_empty());
    assert_eq!(failed.len(), 2, "both failing sources must be reported");
}

/// A source that counts how many times it is searched and always fails with the
/// error its constructor is given — used to pin the fan-out's retry policy.
struct CountingSource {
    calls: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    err: fn() -> patent::Error,
}

#[async_trait::async_trait]
impl SourceAdapter for CountingSource {
    fn id(&self) -> SourceId {
        SourceId::PyPI
    }
    async fn search(&self, _query: &Query) -> patent::Result<Vec<Match>> {
        self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err((self.err)())
    }
}

#[tokio::test]
async fn unavailable_source_is_not_retried_but_transient_is() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // A persistently-unavailable source (a walled search) must be attempted
    // exactly once — retrying only burns 800ms on the same wall.
    let unavailable_calls = Arc::new(AtomicUsize::new(0));
    let unavailable = CountingSource {
        calls: unavailable_calls.clone(),
        err: || patent::Error::Unavailable("walled".to_string()),
    };

    // A transient failure (parse drift, network blip) must still be retried once.
    let transient_calls = Arc::new(AtomicUsize::new(0));
    let transient = CountingSource {
        calls: transient_calls.clone(),
        err: || patent::Error::Parse("transient".to_string()),
    };

    let sources: Vec<Box<dyn SourceAdapter>> = vec![Box::new(unavailable), Box::new(transient)];
    let SearchOutcome { failed, .. } = search_sources(&sources, &query()).await;

    assert_eq!(
        unavailable_calls.load(Ordering::SeqCst),
        1,
        "an Unavailable source must not be retried"
    );
    assert_eq!(
        transient_calls.load(Ordering::SeqCst),
        2,
        "a transient failure must be retried exactly once"
    );
    // Both still land in `failed` — best-effort, never fatal.
    assert_eq!(failed.len(), 2);
}

#[tokio::test]
async fn transient_failure_recovers_on_retry() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_crate_body()))
        .mount(&server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    let sources: Vec<Box<dyn SourceAdapter>> = vec![Box::new(CratesIo::with_base_url(
        reqwest::Client::new(),
        server.uri(),
    ))];

    let SearchOutcome {
        matches,
        reached,
        failed,
    } = search_sources(&sources, &query()).await;

    assert_eq!(matches.len(), 2);
    assert_eq!(reached, vec![SourceId::CratesIo]);
    assert!(failed.is_empty());
}

#[tokio::test]
async fn hanging_source_times_out() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(two_crate_body())
                .set_delay(Duration::from_secs(20)),
        )
        .mount(&server)
        .await;

    let sources: Vec<Box<dyn SourceAdapter>> = vec![Box::new(CratesIo::with_base_url(
        reqwest::Client::new(),
        server.uri(),
    ))];

    let SearchOutcome {
        matches,
        reached,
        failed,
    } = search_sources(&sources, &query()).await;

    assert!(matches.is_empty());
    assert!(reached.is_empty());
    assert_eq!(failed, vec![SourceId::CratesIo]);
}

// ---------------------------------------------------------------------------
// Go (pkg.go.dev HTML scrape)
// ---------------------------------------------------------------------------

fn go_html() -> String {
    r#"<!doctype html><html><body>
      <div class="SearchSnippet">
        <a href="/github.com/spf13/cobra">cobra</a>
        <span data-test-id="snippet-published"><strong>Feb 28, 2026</strong></span>
        <p class="SearchSnippet-synopsis">A Commander for modern Go CLI interactions.</p>
      </div>
    </body></html>"#
        .to_string()
}

#[tokio::test]
async fn go_id_is_go() {
    let src = GoPkgDev::with_base_url(reqwest::Client::new(), "https://pkg.go.dev".to_string());
    assert_eq!(src.id(), SourceId::Go);
}

#[tokio::test]
async fn go_scrapes_packages_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("q", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_string(go_html()))
        .mount(&server)
        .await;

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "cobra");
    assert_eq!(matches[0].source, SourceId::Go);
    assert!(matches[0].url.ends_with("/github.com/spf13/cobra"));
    assert_eq!(
        matches[0].description,
        "A Commander for modern Go CLI interactions."
    );
}

#[tokio::test]
async fn go_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.is_err());
}

#[tokio::test]
async fn go_empty_results_is_ok() {
    let server = MockServer::start().await;
    let html = "<!doctype html><html><body><p>No results.</p></body></html>";
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(&server)
        .await;

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.unwrap().is_empty());
}

/// A real pkg.go.dev zero-result page: large, and carrying the "no matches"
/// gopher. Padded past the 2 KB drift threshold on purpose -- the live page is
/// ~33 KB, so the threshold alone can never tell it apart from drift.
fn go_no_matches_html() -> String {
    format!(
        r#"<!doctype html><html><body>
      <div data-test-id="gopher-message">It looks like there are no matches for your search.</div>
      <!-- {padding} -->
    </body></html>"#,
        padding = "x".repeat(2_500)
    )
}

/// The real pkg.go.dev title anchor: package name as the anchor's own text,
/// then a nested span holding the module path, separated by markup newlines.
fn go_nested_title_html() -> String {
    r#"<!doctype html><html><body>
      <div class="SearchSnippet">
        <h2>
          <a href="/github.com/julienschmidt/httprouter" data-test-id="snippet-title">
            httprouter
            <span class="SearchSnippet-header-path">(github.com/julienschmidt/httprouter)</span>
          </a>
        </h2>
        <p class="SearchSnippet-synopsis">A trie based high performance HTTP request router.</p>
      </div>
    </body></html>"#
        .to_string()
}

#[tokio::test]
async fn go_name_excludes_the_nested_module_path() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(go_nested_title_html()))
        .mount(&server)
        .await;

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    // Not "httprouter (github.com/julienschmidt/httprouter)", and no embedded
    // newlines or run-together indentation from the surrounding markup.
    assert_eq!(matches[0].name, "httprouter");
    assert!(!matches[0].name.contains('\n'));
}

#[tokio::test]
async fn go_genuine_no_matches_is_not_reported_as_drift() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(go_no_matches_html()))
        .mount(&server)
        .await;

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    // Must be Ok(empty), not Err: a source that answered "nothing found" is
    // reached, and reporting it as "not reached" overstates how little we know.
    assert!(src.search(&query()).await.unwrap().is_empty());
}

#[tokio::test]
async fn go_non_trivial_page_without_the_no_matches_marker_is_drift() {
    let server = MockServer::start().await;
    let html = format!(
        "<!doctype html><html><body><div>{}</div></body></html>",
        "x".repeat(2_500)
    );
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(html))
        .mount(&server)
        .await;

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.is_err());
}

#[tokio::test]
async fn go_narrows_to_three_longest_keywords_when_full_query_is_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("q", "ide code spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_string(go_no_matches_html()))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("q", "spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_string(go_html()))
        .expect(1)
        .mount(&server)
        .await;

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&narrowing_query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "cobra");
}

#[tokio::test]
async fn go_falls_back_to_two_longest_keywords() {
    let server = MockServer::start().await;

    for q in ["ide code spell syntax mistakes", "spell syntax mistakes"] {
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("q", q))
            .respond_with(ResponseTemplate::new(200).set_body_string(go_no_matches_html()))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/search"))
        .and(query_param("q", "syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_string(go_html()))
        .expect(1)
        .mount(&server)
        .await;

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&narrowing_query()).await.unwrap();
    assert_eq!(matches.len(), 1);
}

#[tokio::test]
async fn go_all_narrowing_attempts_empty_is_ok_not_err() {
    let server = MockServer::start().await;

    for q in [
        "ide code spell syntax mistakes",
        "spell syntax mistakes",
        "syntax mistakes",
    ] {
        Mock::given(method("GET"))
            .and(path("/search"))
            .and(query_param("q", q))
            .respond_with(ResponseTemplate::new(200).set_body_string(go_no_matches_html()))
            .expect(1)
            .mount(&server)
            .await;
    }

    let src = GoPkgDev::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&narrowing_query()).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Maven Central (Solr JSON)
// ---------------------------------------------------------------------------

fn maven_body() -> serde_json::Value {
    json!({
        "response": {
            "docs": [
                { "g": "com.google.guava", "a": "guava", "versionCount": 50, "timestamp": 1_750_337_811_233_i64 }
            ]
        }
    })
}

#[tokio::test]
async fn maven_id_is_maven() {
    let src = Maven::with_base_url(
        reqwest::Client::new(),
        "https://search.maven.org".to_string(),
    );
    assert_eq!(src.id(), SourceId::Maven);
}

#[tokio::test]
async fn maven_maps_artifacts_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/solrsearch/select"))
        .and(query_param("q", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(maven_body()))
        .mount(&server)
        .await;

    let src = Maven::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "com.google.guava:guava");
    assert_eq!(matches[0].source, SourceId::Maven);
    assert_eq!(
        matches[0].url,
        "https://central.sonatype.com/artifact/com.google.guava/guava"
    );
    // Maven Central's Solr response has no download / star / rank field that
    // is comparable to the popularity signals used by the other sources, so
    // popularity is intentionally left as None.
    assert_eq!(matches[0].popularity, None);
    assert_eq!(matches[0].description, "guava");
}

#[tokio::test]
async fn maven_ignores_version_count_when_mapping_results() {
    // versionCount is still a valid Solr field; the source must not surface
    // it as a popularity signal, because that would mislead the TUI's
    // popularity column.
    let server = MockServer::start().await;
    let body = json!({
        "response": {
            "docs": [
                { "g": "com.google.guava", "a": "guava", "versionCount": 999_999 }
            ]
        }
    });
    Mock::given(method("GET"))
        .and(path("/solrsearch/select"))
        .and(query_param("q", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let src = Maven::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&query()).await.unwrap();
    assert_eq!(matches[0].popularity, None);
}

#[tokio::test]
async fn maven_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/solrsearch/select"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&server)
        .await;

    let src = Maven::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.is_err());
}

#[tokio::test]
async fn maven_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "response": { "docs": [] } });
    Mock::given(method("GET"))
        .and(path("/solrsearch/select"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let src = Maven::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// NuGet (.NET)
// ---------------------------------------------------------------------------

fn nuget_body() -> serde_json::Value {
    json!({
        "data": [
            { "id": "Newtonsoft.Json", "description": "JSON framework for .NET", "totalDownloads": 100 },
            { "id": "NoDescription", "description": "", "totalDownloads": 1 }
        ]
    })
}

#[tokio::test]
async fn nuget_id_is_nuget() {
    let src = NuGet::with_search_url(
        reqwest::Client::new(),
        "https://azuresearch-usnc.nuget.org".to_string(),
    );
    assert_eq!(src.id(), SourceId::NuGet);
}

#[tokio::test]
async fn nuget_maps_packages_and_filters_empty_descriptions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/query"))
        .and(query_param("q", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(nuget_body()))
        .mount(&server)
        .await;

    let src = NuGet::with_search_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&query()).await.unwrap();
    // The empty-description package is filtered out.
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "Newtonsoft.Json");
    assert_eq!(matches[0].source, SourceId::NuGet);
    assert_eq!(
        matches[0].url,
        "https://www.nuget.org/packages/Newtonsoft.Json"
    );
    assert_eq!(matches[0].popularity, Some(100));
}

#[tokio::test]
async fn nuget_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let src = NuGet::with_search_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.is_err());
}

#[tokio::test]
async fn nuget_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "data": [] });
    Mock::given(method("GET"))
        .and(path("/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let src = NuGet::with_search_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// RubyGems
// ---------------------------------------------------------------------------

fn rubygems_body() -> serde_json::Value {
    json!([
        {
            "name": "rails",
            "info": "Full-stack web framework",
            "project_uri": "https://rubygems.org/gems/rails",
            "downloads": 999
        }
    ])
}

#[tokio::test]
async fn rubygems_id_is_rubygems() {
    let src = RubyGems::with_base_url(reqwest::Client::new(), "https://rubygems.org".to_string());
    assert_eq!(src.id(), SourceId::RubyGems);
}

#[tokio::test]
async fn rubygems_maps_gems_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search.json"))
        .and(query_param("query", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rubygems_body()))
        .mount(&server)
        .await;

    let src = RubyGems::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "rails");
    assert_eq!(matches[0].source, SourceId::RubyGems);
    assert_eq!(matches[0].url, "https://rubygems.org/gems/rails");
    assert_eq!(matches[0].description, "Full-stack web framework");
    assert_eq!(matches[0].popularity, Some(999));
}

#[tokio::test]
async fn rubygems_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search.json"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let src = RubyGems::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.is_err());
}

#[tokio::test]
async fn rubygems_empty_results_is_ok() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let src = RubyGems::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// Docker Hub
// ---------------------------------------------------------------------------

fn docker_body() -> serde_json::Value {
    json!({
        "results": [
            { "repo_name": "library/nginx", "short_description": "Official nginx image", "star_count": 200 },
            { "repo_name": "no/desc", "short_description": "", "star_count": 1 }
        ]
    })
}

#[tokio::test]
async fn docker_hub_id_is_docker_hub() {
    let src =
        DockerHub::with_base_url(reqwest::Client::new(), "https://hub.docker.com".to_string());
    assert_eq!(src.id(), SourceId::DockerHub);
}

#[tokio::test]
async fn docker_hub_maps_repos_and_filters_empty_descriptions() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/search/repositories/"))
        .and(query_param("query", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(docker_body()))
        .mount(&server)
        .await;

    let src = DockerHub::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "library/nginx");
    assert_eq!(matches[0].source, SourceId::DockerHub);
    assert_eq!(matches[0].url, "https://hub.docker.com/r/library/nginx");
    assert_eq!(matches[0].popularity, Some(200));
}

#[tokio::test]
async fn docker_hub_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v2/search/repositories/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let src = DockerHub::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.is_err());
}

#[tokio::test]
async fn docker_hub_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "results": [] });
    Mock::given(method("GET"))
        .and(path("/v2/search/repositories/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let src = DockerHub::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.unwrap().is_empty());
}

// ---------------------------------------------------------------------------
// VS Code Marketplace (POST gallery query)
// ---------------------------------------------------------------------------

fn vscode_body() -> serde_json::Value {
    json!({
        "results": [{
            "extensions": [{
                "publisher": { "publisherName": "rust-lang" },
                "extensionName": "rust-analyzer",
                "displayName": "rust-analyzer",
                "shortDescription": "Rust language support",
                "statistics": [{ "statisticName": "install", "value": 12345.0 }],
                "lastUpdated": "2026-08-27T09:58:55.6+00:00"
            }]
        }]
    })
}

#[tokio::test]
async fn vscode_id_is_vscode() {
    let src = VsCodeMarketplace::with_base_url(
        reqwest::Client::new(),
        "https://marketplace.visualstudio.com".to_string(),
    );
    assert_eq!(src.id(), SourceId::VsCodeMarketplace);
}

#[tokio::test]
async fn vscode_maps_extensions_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/_apis/public/gallery/extensionquery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vscode_body()))
        .mount(&server)
        .await;

    let src = VsCodeMarketplace::with_base_url(reqwest::Client::new(), server.uri());
    let matches = src.search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "rust-analyzer");
    assert_eq!(matches[0].source, SourceId::VsCodeMarketplace);
    assert_eq!(
        matches[0].url,
        "https://marketplace.visualstudio.com/items?itemName=rust-lang.rust-analyzer"
    );
    assert_eq!(matches[0].description, "Rust language support");
    assert_eq!(matches[0].popularity, Some(12345));
}

#[tokio::test]
async fn vscode_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/_apis/public/gallery/extensionquery"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let src = VsCodeMarketplace::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.is_err());
}

#[tokio::test]
async fn vscode_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "results": [{ "extensions": [] }] });
    Mock::given(method("POST"))
        .and(path("/_apis/public/gallery/extensionquery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let src = VsCodeMarketplace::with_base_url(reqwest::Client::new(), server.uri());
    assert!(src.search(&query()).await.unwrap().is_empty());
}

//homebrew
#[tokio::test]
async fn test_homebrew_happy_path() {
    let mock_server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/formula.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "name": "ripgrep",
                "desc": "Search tool like grep and The Silver Searcher",
                "homepage": "https://github.com/BurntSushi/ripgrep"
            }
        ])))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/cask.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([
            {
                "token": "google-chrome",
                "desc": "Web browser",
                "homepage": "https://www.google.com/chrome/"
            }
        ])))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let source = Homebrew::with_base_url(client, mock_server.uri());

    // Create a mock query
    let query = Query {
        idea: "A fast search tool".into(),
        keywords: vec!["ripgrep".into()],
    };

    let results = source.search(&query).await.expect("Search should succeed");

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].name, "ripgrep");
    assert!(matches!(results[0].source, SourceId::Homebrew));
}

#[tokio::test]
async fn test_homebrew_empty_results() {
    let mock_server = MockServer::start().await;

    // Mock both APIs returning empty arrays
    Mock::given(method("GET"))
        .and(path("/api/formula.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/api/cask.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .mount(&mock_server)
        .await;

    let client = Client::new();
    let source = Homebrew::with_base_url(client, mock_server.uri());

    let query = Query {
        idea: "Something completely obscure".into(),
        keywords: vec!["doesnotexist123".into()],
    };

    let results = source
        .search(&query)
        .await
        .expect("Search should succeed but be empty");
    assert!(results.is_empty());
}

#[tokio::test]
async fn test_homebrew_server_error() {
    let mock_server = MockServer::start().await;

    // Mock the formula API throwing a 500 Internal Server Error
    Mock::given(method("GET"))
        .and(path("/api/formula.json"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    // We don't need to mock cask.json here, because the adapter should
    // short-circuit and return an Err when the first formula request fails.

    let client = Client::new();
    let source = Homebrew::with_base_url(client, mock_server.uri());

    let query = Query {
        idea: "Testing server crash".into(),
        keywords: vec!["test".into()],
    };

    let result = source.search(&query).await;

    assert!(
        result.is_err(),
        "Expected the adapter to return an Error when hitting a 500 status code"
    );
}

// ---------------------------------------------------------------------------
// Packagist (PHP / Composer)
// ---------------------------------------------------------------------------

fn packagist_for(server: &MockServer) -> Packagist {
    Packagist::with_base_url(test_client(), server.uri())
}

fn packagist_body() -> serde_json::Value {
    json!({
        "results": [
            {
                "name": "laravel/framework",
                "description": "The Laravel Framework.",
                "url": "https://packagist.org/packages/laravel/framework",
                "repository": "https://github.com/laravel/framework",
                "downloads": 543_583_479_u64,
                "favers": 35_281_u64
            },
            {
                "name": "symfony/cache",
                "description": "Provides extended PSR-6, PSR-16 (and tags) implementations",
                "url": "https://packagist.org/packages/symfony/cache",
                "repository": "https://github.com/symfony/cache",
                "downloads": 373_170_138_u64,
                "favers": 4_189_u64
            }
        ],
        "total": 2
    })
}

#[tokio::test]
async fn packagist_id_is_packagist() {
    let src = Packagist::with_base_url(reqwest::Client::new(), "https://packagist.org".to_string());
    assert_eq!(src.id(), SourceId::Packagist);
}

#[tokio::test]
async fn packagist_maps_results_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(packagist_body()))
        .mount(&server)
        .await;

    let matches = packagist_for(&server).search(&query()).await.unwrap();

    assert_eq!(matches.len(), 2);

    let first = &matches[0];
    assert_eq!(first.name, "laravel/framework");
    assert_eq!(first.source, SourceId::Packagist);
    assert_eq!(
        first.url,
        "https://packagist.org/packages/laravel/framework"
    );
    assert_eq!(first.description, "The Laravel Framework.");
    assert_eq!(first.popularity, Some(543_583_479));
    assert_eq!(first.similarity, 0.0);

    assert_eq!(matches[1].name, "symfony/cache");
    assert_eq!(matches[1].popularity, Some(373_170_138));
}

#[tokio::test]
async fn packagist_sends_joined_keywords_and_per_page() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .and(query_param("q", "async runtime"))
        .and(query_param("per_page", "15"))
        .respond_with(ResponseTemplate::new(200).set_body_json(packagist_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = packagist_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
}

#[tokio::test]
async fn packagist_falls_back_to_favers_when_downloads_zero() {
    let server = MockServer::start().await;
    let body = json!({
        "results": [
            {
                "name": "fresh/package",
                "description": "Brand new, no downloads yet.",
                "url": "https://packagist.org/packages/fresh/package",
                "downloads": 0,
                "favers": 42
            }
        ],
        "total": 1
    });
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = packagist_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].popularity, Some(42));
}

#[tokio::test]
async fn packagist_missing_popularity_becomes_none() {
    let server = MockServer::start().await;
    let body = json!({
        "results": [
            {
                "name": "obscure/pkg",
                "description": "no counts here",
                "url": "https://packagist.org/packages/obscure/pkg"
            }
        ],
        "total": 1
    });
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = packagist_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].popularity, None);
}

#[tokio::test]
async fn packagist_empty_results_is_ok_not_error() {
    let server = MockServer::start().await;
    let body = json!({ "results": [], "total": 0 });
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = packagist_for(&server).search(&query()).await.unwrap();
    assert!(matches.is_empty());
}

#[tokio::test]
async fn packagist_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    let result = packagist_for(&server).search(&query()).await;
    assert!(result.is_err(), "a 5xx response must surface as an error");
}

// ---------------------------------------------------------------------------
// Hex (Erlang/Elixir)
// ---------------------------------------------------------------------------

fn hex_for(server: &MockServer) -> Hex {
    Hex::with_base_url(test_client(), server.uri())
}

fn hex_body() -> serde_json::Value {
    json!([
        {
            "name": "phoenix",
            "updated_at": "2026-03-24T16:51:48.689517Z",
            "html_url": "https://hex.pm/packages/phoenix",
            "meta": { "description": "Productive web framework that does not compromise speed or maintainability." },
            "downloads": { "all": 50_000_000_u64, "recent": 1_000_000_u64, "week": 250_000_u64 }
        },
        {
            "name": "cachex",
            "html_url": "https://hex.pm/packages/cachex",
            "meta": { "description": "A powerful caching library for Elixir." },
            "downloads": { "all": 12_345_u64 }
        }
    ])
}

#[tokio::test]
async fn hex_id_is_hex() {
    let src = Hex::with_base_url(reqwest::Client::new(), "https://hex.pm".to_string());
    assert_eq!(src.id(), SourceId::Hex);
}

#[tokio::test]
async fn hex_maps_packages_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/packages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(hex_body()))
        .mount(&server)
        .await;

    let matches = hex_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);

    let first = &matches[0];
    assert_eq!(first.name, "phoenix");
    assert_eq!(first.source, SourceId::Hex);
    assert_eq!(first.url, "https://hex.pm/packages/phoenix");
    assert_eq!(
        first.description,
        "Productive web framework that does not compromise speed or maintainability."
    );
    assert_eq!(first.popularity, Some(50_000_000));
    assert_eq!(first.similarity, 0.0);

    assert_eq!(matches[1].name, "cachex");
    assert_eq!(matches[1].popularity, Some(12_345));
}

#[tokio::test]
async fn hex_searches_each_keyword_on_its_own_and_dedupes_across_terms() {
    // Hex's grammar only honours a single bare term: a joined multi-word query
    // returns the unfiltered catalogue. Each keyword must go out alone, and a
    // package hit by two terms must appear once.
    let server = MockServer::start().await;
    for term in ["async", "runtime"] {
        Mock::given(method("GET"))
            .and(path("/api/packages"))
            .and(query_param("search", term))
            .respond_with(ResponseTemplate::new(200).set_body_json(hex_body()))
            .expect(1)
            .mount(&server)
            .await;
    }

    let matches = hex_for(&server).search(&query()).await.unwrap();
    assert_eq!(
        matches.len(),
        2,
        "the same two packages from both terms, deduped"
    );
}

#[tokio::test]
async fn hex_searches_only_the_three_longest_keywords() {
    let server = MockServer::start().await;
    // Lengths: ide=3, code=4, spell=5, syntax=6, mistakes=8 -> the three
    // longest are mistakes, syntax, spell; ide and code must never be sent.
    for term in ["mistakes", "syntax", "spell"] {
        Mock::given(method("GET"))
            .and(path("/api/packages"))
            .and(query_param("search", term))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
            .expect(1)
            .mount(&server)
            .await;
    }
    for term in ["ide", "code", "ide code spell syntax mistakes"] {
        Mock::given(method("GET"))
            .and(path("/api/packages"))
            .and(query_param("search", term))
            .respond_with(ResponseTemplate::new(200).set_body_json(hex_body()))
            .expect(0)
            .mount(&server)
            .await;
    }

    assert!(hex_for(&server)
        .search(&narrowing_query())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn hex_missing_description_becomes_empty() {
    let server = MockServer::start().await;
    let body = json!([
        { "name": "bare", "html_url": "https://hex.pm/packages/bare", "downloads": { "all": 5 } }
    ]);
    Mock::given(method("GET"))
        .and(path("/api/packages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = hex_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].description, "");
}

#[tokio::test]
async fn hex_zero_or_missing_downloads_becomes_none_and_url_falls_back() {
    let server = MockServer::start().await;
    let body = json!([
        { "name": "mystery", "meta": { "description": "no link, no downloads" }, "downloads": { "all": 0 } },
        { "name": "ghost", "meta": { "description": "no downloads block at all" } }
    ]);
    Mock::given(method("GET"))
        .and(path("/api/packages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = hex_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].popularity, None);
    assert_eq!(matches[0].url, format!("{}/packages/mystery", server.uri()));
    assert_eq!(matches[1].popularity, None);
}

#[tokio::test]
async fn hex_empty_results_is_ok_not_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/packages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    assert!(hex_for(&server).search(&query()).await.unwrap().is_empty());
}

#[tokio::test]
async fn hex_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/packages"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(hex_for(&server).search(&query()).await.is_err());
}

// ---------------------------------------------------------------------------
// Artifact Hub (Cloud Native package index)
// ---------------------------------------------------------------------------

fn artifacthub_for(server: &MockServer) -> ArtifactHub {
    ArtifactHub::with_base_url(test_client(), server.uri())
}

fn artifacthub_body() -> serde_json::Value {
    json!({
        "packages": [
            {
                "name": "prometheus",
                "ts": 1_785_445_235_i64,
                "description": "Prometheus monitoring system and time series database",
                "stars": 4200,
                "repository": {
                    "name": "prometheus-community",
                    "kind": 0,
                    "url": "https://prometheus-community.github.io/helm-charts"
                }
            },
            {
                "name": "trace-dns",
                "stars": 0,
                "repository": {
                    "name": "gadgets",
                    "kind": 22,
                    "url": "https://github.com/inspektor-gadget/inspektor-gadget"
                }
            }
        ]
    })
}

#[tokio::test]
async fn artifacthub_id_is_artifact_hub() {
    let src =
        ArtifactHub::with_base_url(reqwest::Client::new(), "https://artifacthub.io".to_string());
    assert_eq!(src.id(), SourceId::ArtifactHub);
}

#[tokio::test]
async fn artifacthub_maps_packages_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/packages/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(artifacthub_body()))
        .mount(&server)
        .await;

    let matches = artifacthub_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);

    let first = &matches[0];
    assert_eq!(first.name, "prometheus");
    assert_eq!(first.source, SourceId::ArtifactHub);
    assert_eq!(
        first.url,
        format!(
            "{}/packages/helm/prometheus-community/prometheus",
            server.uri()
        )
    );
    assert_eq!(
        first.description,
        "Prometheus monitoring system and time series database"
    );
    assert_eq!(first.popularity, Some(4200));
    assert_eq!(first.similarity, 0.0);

    let second = &matches[1];
    assert_eq!(second.name, "trace-dns");
    assert_eq!(
        second.url,
        format!(
            "{}/packages/inspektor-gadget/gadgets/trace-dns",
            server.uri()
        )
    );
    assert_eq!(second.description, "");
    assert_eq!(second.popularity, None);
}

#[tokio::test]
async fn artifacthub_sends_joined_keywords_as_query() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/packages/search"))
        .and(query_param("ts_query_web", "async runtime"))
        .and(query_param("limit", "15"))
        .and(header_exists("user-agent"))
        .respond_with(ResponseTemplate::new(200).set_body_json(artifacthub_body()))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(
        artifacthub_for(&server)
            .search(&query())
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn artifacthub_unknown_kind_falls_back_to_repository_url() {
    let server = MockServer::start().await;
    let body = json!({
        "packages": [
            {
                "name": "future-thing",
                "description": "kind this adapter has no slug for yet",
                "stars": 3,
                "repository": {
                    "name": "some-repo",
                    "kind": 9999,
                    "url": "https://example.com/some-repo"
                }
            }
        ]
    });
    Mock::given(method("GET"))
        .and(path("/api/v1/packages/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = artifacthub_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].url, "https://example.com/some-repo");
}

#[tokio::test]
async fn artifacthub_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "packages": [] });
    Mock::given(method("GET"))
        .and(path("/api/v1/packages/search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    assert!(artifacthub_for(&server)
        .search(&query())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn artifacthub_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/packages/search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(artifacthub_for(&server).search(&query()).await.is_err());
}

// ---------------------------------------------------------------------------
// AUR (Arch User Repository, RPC v5 JSON)
// ---------------------------------------------------------------------------

fn aur_for(server: &MockServer) -> Aur {
    Aur::with_base_url(test_client(), server.uri())
}

fn aur_body() -> serde_json::Value {
    json!({
        "resultcount": 2,
        "results": [
            {
                "Name": "ccache",
                "LastModified": 1_778_477_360_i64,
                "Description": "Compiler cache that speeds up recompilation",
                "URL": "https://ccache.dev",
                "NumVotes": 120,
                "Popularity": 3.5,
                "Maintainer": "someone",
                "Version": "4.10-1"
            },
            {
                "Name": "sccache-git",
                "Description": null,
                "URL": null,
                "NumVotes": 0,
                "Popularity": 0,
                "Maintainer": null,
                "Version": "0.8.1-1"
            }
        ]
    })
}

#[tokio::test]
async fn aur_id_is_aur() {
    let src = Aur::with_base_url(
        reqwest::Client::new(),
        "https://aur.archlinux.org".to_string(),
    );
    assert_eq!(src.id(), SourceId::Aur);
}

#[tokio::test]
async fn aur_maps_packages_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(aur_body()))
        .mount(&server)
        .await;

    let matches = aur_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);

    let ccache = &matches[0];
    assert_eq!(ccache.name, "ccache");
    assert_eq!(ccache.source, SourceId::Aur);
    assert_eq!(ccache.url, format!("{}/packages/ccache", server.uri()));
    assert_eq!(
        ccache.description,
        "Compiler cache that speeds up recompilation"
    );
    assert_eq!(ccache.popularity, Some(120));
    assert_eq!(ccache.similarity, 0.0);

    let sccache = &matches[1];
    assert_eq!(sccache.name, "sccache-git");
    assert_eq!(
        sccache.url,
        format!("{}/packages/sccache-git", server.uri())
    );
    assert_eq!(sccache.description, "");
    assert_eq!(sccache.popularity, None);
}

#[tokio::test]
async fn aur_sends_search_query_params() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .and(query_param("v", "5"))
        .and(query_param("type", "search"))
        .and(query_param("by", "name-desc"))
        .and(query_param("arg", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(aur_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = aur_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
}

#[tokio::test]
async fn aur_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "resultcount": 0, "results": [] });
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    assert!(aur_for(&server).search(&query()).await.unwrap().is_empty());
}

#[tokio::test]
async fn aur_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(aur_for(&server).search(&query()).await.is_err());
}

/// The RPC refuses over-broad terms with **HTTP 200** and an error body. Name
/// matching survives terms that name+description matching is refused for, so
/// the refusal costs recall for that step, not the whole source.
#[tokio::test]
async fn aur_too_many_results_retries_against_names_only() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .and(query_param("by", "name-desc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "error": "Too many package results.",
            "resultcount": 0,
            "results": [],
            "type": "error"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .and(query_param("by", "name"))
        .respond_with(ResponseTemplate::new(200).set_body_json(aur_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = aur_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].name, "ccache");
}

/// An RPC error the fallback cannot get past must reach the caller as a failed
/// source. Decoding only `results` would report the refusal as a successful
/// search that found nothing — the one thing this tool must never claim.
#[tokio::test]
async fn aur_error_body_fails_the_source_instead_of_reporting_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "error": "Incorrect request type specified.",
            "resultcount": 0,
            "results": [],
            "type": "error"
        })))
        .mount(&server)
        .await;

    let err = aur_for(&server).search(&query()).await.unwrap_err();
    assert!(
        matches!(err, patent::Error::Unavailable(_)),
        "a 200 carrying an RPC error must not decode to an empty result, got: {err:?}"
    );
}

/// A single broad keyword matches hundreds of unranked AUR packages, so each
/// step keeps only the most voted-for handful. Without the cap, narrowing down
/// to one keyword would flood the ranker with the tail.
#[tokio::test]
async fn aur_keeps_only_the_top_voted_packages() {
    let server = MockServer::start().await;
    let results: Vec<_> = (0..25)
        .map(|i| {
            json!({
                "Name": format!("pkg-{i}"),
                "Description": "a package",
                "NumVotes": i,
            })
        })
        .collect();
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(json!({ "resultcount": 25, "results": results })),
        )
        .mount(&server)
        .await;

    let matches = aur_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 20, "each step is capped");
    assert_eq!(matches[0].name, "pkg-24", "the most voted-for comes first");
    assert_eq!(matches[19].name, "pkg-5");
}

// ---------------------------------------------------------------------------
// Nixpkgs (NixOS Elasticsearch)
// ---------------------------------------------------------------------------

fn nixpkgs_for(server: &MockServer) -> Nixpkgs {
    Nixpkgs::with_base_url(reqwest::Client::new(), server.uri())
}

fn nixpkgs_body() -> serde_json::Value {
    json!({
        "hits": {
            "hits": [
                {
                    "_source": {
                        "package_attr_name": "ripgrep",
                        "package_pname": "ripgrep",
                        "package_description": "A utility that combines the usability of The Silver Searcher with the raw speed of grep",
                        "package_homepage": ["https://github.com/BurntSushi/ripgrep"],
                        "package_pversion": "14.1.0"
                    }
                },
                {
                    "_source": {
                        "package_attr_name": "fd",
                        "package_pname": "fd",
                        "package_description": "A simple, fast and user-friendly alternative to find",
                        "package_homepage": ["https://github.com/sharkdp/fd"],
                        "package_pversion": "10.2.0"
                    }
                }
            ]
        }
    })
}

#[tokio::test]
async fn nixpkgs_id_is_nixpkgs() {
    let src = Nixpkgs::with_base_url(
        reqwest::Client::new(),
        "https://search.nixos.org".to_string(),
    );
    assert_eq!(src.id(), SourceId::Nixpkgs);
}

#[tokio::test]
async fn nixpkgs_search_returns_matches() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/backend/latest-\d+-nixos-.*/_search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(nixpkgs_body()))
        .mount(&server)
        .await;

    let matches = nixpkgs_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 2);

    let first = &matches[0];
    assert_eq!(first.name, "ripgrep (v14.1.0)");
    assert_eq!(first.source, SourceId::Nixpkgs);
    assert_eq!(first.url, "https://github.com/BurntSushi/ripgrep");
    assert_eq!(
        first.description,
        "A utility that combines the usability of The Silver Searcher with the raw speed of grep"
    );
    assert_eq!(first.popularity, None);
    assert_eq!(first.similarity, 0.0);

    let second = &matches[1];
    assert_eq!(second.name, "fd (v10.2.0)");
    assert_eq!(second.url, "https://github.com/sharkdp/fd");
    assert_eq!(
        second.description,
        "A simple, fast and user-friendly alternative to find"
    );
}

// The backend 401s anonymous callers, so a request without this header returns
// nothing at all — the failure that shipped in 0.9.0. The mock only asserts the
// header is present, not its value: `live_nixpkgs` is what proves the
// credentials are still accepted. This exists so dropping `.basic_auth(..)`
// fails in PR CI rather than waiting for the nightly live run.
#[tokio::test]
async fn nixpkgs_sends_authorization_header() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/backend/latest-\d+-nixos-.*/_search"))
        .and(header_exists("authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(nixpkgs_body()))
        .mount(&server)
        .await;

    let matches = nixpkgs_for(&server)
        .search(&query())
        .await
        .expect("request without an Authorization header did not match the mock");
    assert_eq!(matches.len(), 2);
}

#[tokio::test]
async fn nixpkgs_empty_results() {
    let server = MockServer::start().await;
    let body = json!({ "hits": { "hits": [] } });
    Mock::given(method("POST"))
        .and(path_regex(r"/backend/latest-\d+-nixos-.*/_search"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = nixpkgs_for(&server).search(&query()).await.unwrap();
    assert!(matches.is_empty());
}

#[tokio::test]
async fn nixpkgs_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/backend/latest-\d+-nixos-.*/_search"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(nixpkgs_for(&server).search(&query()).await.is_err());
}

#[tokio::test]
async fn nixpkgs_malformed_body_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path_regex(r"/backend/latest-\d+-nixos-.*/_search"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;

    let result = nixpkgs_for(&server).search(&query()).await;
    assert!(
        result.is_err(),
        "an unparseable body must surface as an error"
    );
}

// ---------------------------------------------------------------------------
// JetBrains Marketplace
// ---------------------------------------------------------------------------

fn jetbrains_for(server: &MockServer) -> JetBrains {
    JetBrains::with_base_url(reqwest::Client::new(), server.uri())
}

fn jetbrains_body() -> serde_json::Value {
    json!({
        "plugins": [{
            "id": 164,
            "xmlId": "IdeaVIM",
            "link": "/plugin/164-ideavim",
            "name": "IdeaVim",
            "preview": "Bring the power of Vim to your JetBrains IDE.",
            "downloads": 21_803_835_u64,
            "pricingModel": "FREE",
            "cdate": 1_785_747_916_000_i64,
            "rating": 4.44,
            "tags": ["Editor", "Keymap"],
            "vendor": { "name": "JetBrains s.r.o.", "isVerified": true }
        }],
        "total": 1,
        "correctedQuery": ""
    })
}

#[tokio::test]
async fn jetbrains_id_is_jetbrains() {
    let src = JetBrains::with_base_url(
        reqwest::Client::new(),
        "https://plugins.jetbrains.com".to_string(),
    );
    assert_eq!(src.id(), SourceId::JetBrains);
}

#[tokio::test]
async fn jetbrains_maps_plugins_into_matches() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jetbrains_body()))
        .mount(&server)
        .await;

    let matches = jetbrains_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    let m = &matches[0];
    assert_eq!(m.name, "IdeaVim");
    assert_eq!(m.source, SourceId::JetBrains);
    // Built against the mock server, proving the relative `link` join, not the
    // live `plugins.jetbrains.com` host.
    assert_eq!(m.url, format!("{}/plugin/164-ideavim", server.uri()));
    assert_eq!(
        m.description,
        "Bring the power of Vim to your JetBrains IDE."
    );
    assert_eq!(m.popularity, Some(21_803_835));
    // Epoch-millis `cdate`, normalised to whole-second RFC 3339 UTC.
    assert_eq!(m.last_updated.as_deref(), Some("2026-08-03T09:05:16Z"));
}

#[tokio::test]
async fn jetbrains_description_falls_back_to_name_when_preview_missing() {
    let server = MockServer::start().await;
    let body = json!({
        "plugins": [
            {
                "name": "NoPreview",
                "link": "/plugin/1-no-preview"
            },
            {
                "name": "BlankPreview",
                "link": "/plugin/5-blank-preview",
                "preview": "   "
            }
        ],
        "total": 2,
        "correctedQuery": ""
    });
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = jetbrains_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches[0].description, "NoPreview");
    // A `preview` present but whitespace-only must fall back the same as an
    // absent one.
    assert_eq!(matches[1].description, "BlankPreview");
}

#[tokio::test]
async fn jetbrains_truncates_long_descriptions() {
    let server = MockServer::start().await;
    let long_preview = "a".repeat(200);
    let body = json!({
        "plugins": [{
            "name": "LongDescPlugin",
            "link": "/plugin/2-long-desc",
            "preview": long_preview
        }],
        "total": 1,
        "correctedQuery": ""
    });
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = jetbrains_for(&server).search(&query()).await.unwrap();
    let desc = &matches[0].description;
    assert_eq!(desc.chars().count(), 121);
    assert!(desc.ends_with('…'));
}

#[tokio::test]
async fn jetbrains_truncates_long_multibyte_descriptions() {
    // "a".repeat(200) is pure ASCII and proves nothing about char-safety —
    // truncating by byte offset on a multibyte string would panic or split a
    // character. Use a non-ASCII repeat to prove `chars().take` is doing the
    // truncation, not a byte slice.
    let server = MockServer::start().await;
    let long_preview = "é".repeat(200);
    let body = json!({
        "plugins": [{
            "name": "MultibyteDescPlugin",
            "link": "/plugin/7-multibyte-desc",
            "preview": long_preview
        }],
        "total": 1,
        "correctedQuery": ""
    });
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = jetbrains_for(&server).search(&query()).await.unwrap();
    let desc = &matches[0].description;
    assert_eq!(desc.chars().count(), 121);
    assert!(desc.ends_with('…'));
    assert!(desc.starts_with('é'));
}

#[tokio::test]
async fn jetbrains_skips_plugin_with_missing_name() {
    let server = MockServer::start().await;
    let body = json!({
        "plugins": [
            { "link": "/plugin/3-no-name", "preview": "no name here" },
            { "name": "", "link": "/plugin/4-empty-name" }
        ],
        "total": 2,
        "correctedQuery": ""
    });
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    assert!(jetbrains_for(&server)
        .search(&query())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn jetbrains_skips_plugin_with_missing_link() {
    let server = MockServer::start().await;
    let body = json!({
        "plugins": [
            { "name": "NoLink" },
            { "name": "EmptyLink", "link": "" },
            { "name": "BareHostLink", "link": "plugin/5-bare" }
        ],
        "total": 3,
        "correctedQuery": ""
    });
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    assert!(jetbrains_for(&server)
        .search(&query())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn jetbrains_wrong_type_cdate_degrades_to_none_not_error() {
    // The `lenient` contract: a type change on the date field (a string where a
    // number used to be) must yield `last_updated: None` and still return the
    // plugin, not fail the whole response.
    let server = MockServer::start().await;
    let body = json!({
        "plugins": [{
            "name": "BadDate",
            "link": "/plugin/6-bad-date",
            "cdate": "not-a-number"
        }],
        "total": 1,
        "correctedQuery": ""
    });
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let matches = jetbrains_for(&server).search(&query()).await.unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].last_updated, None);
}

#[tokio::test]
async fn jetbrains_empty_results_is_ok() {
    let server = MockServer::start().await;
    let body = json!({ "plugins": [], "total": 0, "correctedQuery": "" });
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    assert!(jetbrains_for(&server)
        .search(&query())
        .await
        .unwrap()
        .is_empty());
}

#[tokio::test]
async fn jetbrains_server_error_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&server)
        .await;

    assert!(jetbrains_for(&server).search(&query()).await.is_err());
}

#[tokio::test]
async fn jetbrains_malformed_body_is_propagated() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>not json</html>"))
        .mount(&server)
        .await;

    let result = jetbrains_for(&server).search(&query()).await;
    assert!(
        result.is_err(),
        "an unparseable body must surface as an error"
    );
}

#[tokio::test]
async fn jetbrains_missing_plugins_key_is_propagated() {
    // Syntactically valid JSON, but without the `plugins` key the response
    // can't be deserialized into `SearchResponse` — must be an `Err`, not a
    // silent empty result.
    let server = MockServer::start().await;
    let body = json!({ "total": 0, "correctedQuery": "" });
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(&server)
        .await;

    let result = jetbrains_for(&server).search(&query()).await;
    assert!(
        result.is_err(),
        "a body missing the `plugins` key must surface as an error"
    );
}

// ── progressive keyword narrowing (Fix 1) ───────────────────────────────────
//
// `searchPlugins` ANDs every content word, so a realistic multi-keyword idea
// returns zero hits; the adapter must narrow to fewer, longer keywords until
// something comes back. These use distinct keyword lengths throughout so the
// "longest 3"/"longest 2" selection is unambiguous.

fn narrowing_keywords() -> Vec<String> {
    // Lengths: ide=3, code=4, spell=5, syntax=6, mistakes=8.
    ["ide", "code", "spell", "syntax", "mistakes"]
        .iter()
        .map(|s| s.to_string())
        .collect()
}

fn narrowing_query() -> Query {
    Query {
        idea: "an ide plugin that highlights spelling mistakes in code comments".to_string(),
        keywords: narrowing_keywords(),
    }
}

#[tokio::test]
async fn jetbrains_narrows_to_three_longest_keywords_when_full_query_is_empty() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .and(query_param("search", "ide code spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "plugins": [] })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .and(query_param("search", "spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jetbrains_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = jetbrains_for(&server)
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "IdeaVim");
}

#[tokio::test]
async fn jetbrains_falls_back_to_two_longest_keywords() {
    let server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .and(query_param("search", "ide code spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "plugins": [] })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .and(query_param("search", "spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "plugins": [] })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .and(query_param("search", "syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(jetbrains_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = jetbrains_for(&server)
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "IdeaVim");
}

#[tokio::test]
async fn jetbrains_all_narrowing_attempts_empty_is_ok_not_err() {
    let server = MockServer::start().await;

    for q in [
        "ide code spell syntax mistakes",
        "spell syntax mistakes",
        "syntax mistakes",
    ] {
        Mock::given(method("GET"))
            .and(path("/api/searchPlugins"))
            .and(query_param("search", q))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "plugins": [] })))
            .expect(1)
            .mount(&server)
            .await;
    }

    let matches = jetbrains_for(&server)
        .search(&narrowing_query())
        .await
        .unwrap();
    assert!(matches.is_empty());
}

#[tokio::test]
async fn jetbrains_short_keyword_set_issues_only_one_request() {
    // With ≤3 keywords, every narrowing attempt ("3 longest", "2 longest")
    // is textually identical to the full join — the adapter must dedup them
    // away rather than resend the same query twice more. `expect(1)` proves
    // it: without the dedup, this mock would receive three identical
    // requests (an empty result never breaks the retry loop early).
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/searchPlugins"))
        .and(query_param("search", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "plugins": [] })))
        .expect(1)
        .mount(&server)
        .await;

    let matches = jetbrains_for(&server).search(&query()).await.unwrap();
    assert!(matches.is_empty());
}

// ── last_updated (#29) ──────────────────────────────────────────────────────
//
// Three properties per adapter that reports a date:
//   1. the registry's own format is normalised to whole-second RFC 3339 UTC;
//   2. an entry with no date yields `None` rather than a fabricated one;
//   3. an unparseable date yields `None` *and still returns its matches* — a
//      registry changing its timestamp format must not take out the source.
//
// The fixture values are the real shapes probed from each live API (microsecond
// and millisecond RFC 3339, epoch seconds, epoch millis, a rendered date), so
// these pin the formats actually in play.

/// Serve `body` at `path`, run `search`, return the matches.
macro_rules! matches_from {
    ($server:expr, $path:expr, $body:expr, $src:expr) => {{
        Mock::given(method("GET"))
            .and(path($path))
            .respond_with(ResponseTemplate::new(200).set_body_json($body))
            .mount(&$server)
            .await;
        $src.search(&query()).await.unwrap()
    }};
}

#[tokio::test]
async fn crates_io_normalises_last_updated_and_omits_it_when_absent() {
    let server = MockServer::start().await;
    let m = matches_from!(
        server,
        "/api/v1/crates",
        two_crate_body(),
        source_for(&server)
    );
    assert_eq!(m[0].last_updated.as_deref(), Some("2026-05-15T06:13:41Z"));
    assert_eq!(m[1].last_updated, None, "no updated_at in the fixture");
}

#[tokio::test]
async fn github_uses_pushed_at_for_last_updated() {
    let server = MockServer::start().await;
    let m = matches_from!(
        server,
        "/search/repositories",
        github_body(),
        github_for(&server)
    );
    assert_eq!(m[0].last_updated.as_deref(), Some("2026-07-31T18:49:43Z"));
    assert_eq!(m[1].last_updated, None);
}

#[tokio::test]
async fn npm_normalises_millisecond_dates() {
    let server = MockServer::start().await;
    let m = matches_from!(server, "/-/v1/search", npm_body(), npm_for(&server));
    assert_eq!(m[0].last_updated.as_deref(), Some("2026-07-21T15:41:28Z"));
    assert_eq!(m[1].last_updated, None);
}

#[tokio::test]
async fn hex_normalises_microsecond_dates() {
    let server = MockServer::start().await;
    let m = matches_from!(server, "/api/packages", hex_body(), hex_for(&server));
    assert_eq!(m[0].last_updated.as_deref(), Some("2026-03-24T16:51:48Z"));
    assert_eq!(m[1].last_updated, None);
}

#[tokio::test]
async fn maven_converts_epoch_millis() {
    let server = MockServer::start().await;
    let src = Maven::with_base_url(Client::new(), server.uri());
    let m = matches_from!(server, "/solrsearch/select", maven_body(), src);
    // Millis, not seconds — reading 1_750_337_811_233 as seconds lands in year 57000.
    assert_eq!(m[0].last_updated.as_deref(), Some("2025-06-19T12:56:51Z"));
}

#[tokio::test]
async fn aur_converts_epoch_seconds() {
    let server = MockServer::start().await;
    let m = matches_from!(server, "/rpc/", aur_body(), aur_for(&server));
    assert_eq!(m[0].last_updated.as_deref(), Some("2026-05-11T05:29:20Z"));
    assert_eq!(m[1].last_updated, None);
}

#[tokio::test]
async fn artifacthub_converts_epoch_seconds() {
    let server = MockServer::start().await;
    let m = matches_from!(
        server,
        "/api/v1/packages/search",
        artifacthub_body(),
        artifacthub_for(&server)
    );
    assert_eq!(m[0].last_updated.as_deref(), Some("2026-07-30T21:00:35Z"));
    assert_eq!(m[1].last_updated, None);
}

#[tokio::test]
async fn go_parses_the_rendered_publication_date() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search"))
        .respond_with(ResponseTemplate::new(200).set_body_string(go_html()))
        .mount(&server)
        .await;
    let m = GoPkgDev::with_base_url(Client::new(), server.uri())
        .search(&query())
        .await
        .unwrap();
    assert_eq!(m[0].last_updated.as_deref(), Some("2026-02-28T00:00:00Z"));
}

#[tokio::test]
async fn a_malformed_date_never_fails_its_source() {
    // Each adapter that reads a date, fed a broken value in that field only.
    // Two distinct failure modes are covered: a *parse* failure (a well-typed
    // string that isn't a date) and a *type* failure (a number where a string
    // was, or vice versa). The second is the one `#[serde(default)]` alone does
    // not survive — it aborts deserialisation of the whole response — which is
    // why the date fields also go through `freshness::lenient`.
    type Build = fn(String) -> Box<dyn SourceAdapter>;
    let cases: Vec<(&str, serde_json::Value, Build)> = vec![
        (
            "/api/v1/crates",
            {
                let mut b = two_crate_body();
                b["crates"][0]["updated_at"] = json!("last tuesday"); // unparseable
                b
            },
            |u| Box::new(CratesIo::with_base_url(test_client(), u)),
        ),
        (
            "/search/repositories",
            {
                let mut b = github_body();
                b["items"][0]["pushed_at"] = json!(""); // empty
                b
            },
            |u| Box::new(GitHub::with_base_url(test_client(), u)),
        ),
        (
            "/-/v1/search",
            {
                let mut b = npm_body();
                b["objects"][0]["package"]["date"] = json!(1_755_000_000_i64); // number, was a string
                b
            },
            |u| Box::new(Npm::with_base_url(test_client(), u)),
        ),
        (
            "/api/packages",
            {
                let mut b = hex_body();
                b[0]["updated_at"] = json!({ "iso": "2026-03-24" }); // object, was a string
                b
            },
            |u| Box::new(Hex::with_base_url(test_client(), u)),
        ),
        (
            "/solrsearch/select",
            {
                let mut b = maven_body();
                b["response"]["docs"][0]["timestamp"] = json!("1750337811233"); // string, was a number
                b
            },
            |u| Box::new(Maven::with_base_url(test_client(), u)),
        ),
        (
            "/rpc/",
            {
                let mut b = aur_body();
                b["results"][0]["LastModified"] = json!(i64::MAX); // out of range
                b
            },
            |u| Box::new(Aur::with_base_url(test_client(), u)),
        ),
        (
            "/api/v1/packages/search",
            {
                let mut b = artifacthub_body();
                b["packages"][0]["ts"] = json!("not-a-number"); // string, was a number
                b
            },
            |u| Box::new(ArtifactHub::with_base_url(test_client(), u)),
        ),
        (
            "/api/searchPlugins",
            {
                let mut b = jetbrains_body();
                b["plugins"][0]["cdate"] = json!("not-a-number"); // string, was a number
                b
            },
            |u| Box::new(JetBrains::with_base_url(test_client(), u)),
        ),
    ];

    for (route, body, build) in cases {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path(route))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;

        let matches = build(server.uri())
            .search(&query())
            .await
            .unwrap_or_else(|e| panic!("{route}: a broken date must not fail the source: {e}"));

        assert!(
            !matches.is_empty(),
            "{route}: the matches themselves must still come through"
        );
        assert!(
            matches.iter().all(|m| m.last_updated.is_none()),
            "{route}: a broken date must degrade to None, not a fabricated value"
        );
    }
}

#[tokio::test]
async fn vscode_normalises_a_single_digit_fraction_and_explicit_offset() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/_apis/public/gallery/extensionquery"))
        .respond_with(ResponseTemplate::new(200).set_body_json(vscode_body()))
        .mount(&server)
        .await;
    let m = VsCodeMarketplace::with_base_url(reqwest::Client::new(), server.uri())
        .search(&query())
        .await
        .unwrap();
    // `2026-08-27T09:58:55.6+00:00` — the only source seen writing a one-digit
    // fraction and a numeric zero offset rather than `Z`.
    assert_eq!(m[0].last_updated.as_deref(), Some("2026-08-27T09:58:55Z"));
}

/// The gallery only returns `lastUpdated` under `flags: 914`; a narrower flag
/// set drops the field entirely, which must read as "no date", not fail.
#[tokio::test]
async fn vscode_survives_a_missing_or_broken_date() {
    for body in [
        {
            let mut b = vscode_body();
            b["results"][0]["extensions"][0]
                .as_object_mut()
                .unwrap()
                .remove("lastUpdated");
            b
        },
        {
            let mut b = vscode_body();
            b["results"][0]["extensions"][0]["lastUpdated"] = json!(1_756_288_735_i64);
            b
        },
    ] {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/_apis/public/gallery/extensionquery"))
            .respond_with(ResponseTemplate::new(200).set_body_json(body))
            .mount(&server)
            .await;
        let m = VsCodeMarketplace::with_base_url(reqwest::Client::new(), server.uri())
            .search(&query())
            .await
            .expect("a missing or broken date must not fail the source");
        assert_eq!(m.len(), 1, "the match itself must still come through");
        assert_eq!(m[0].last_updated, None);
    }
}

/// pkg.go.dev is scraped, so its date drifts differently: the element can move,
/// vanish, or start rendering a format we don't know.
#[tokio::test]
async fn go_survives_an_unparseable_rendered_date() {
    for html in [
        go_html().replace("Feb 28, 2026", "Smarch 40, 2026"),
        go_html().replace("Feb 28, 2026", ""),
        go_html().replace("snippet-published", "snippet-renamed"),
    ] {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/search"))
            .respond_with(ResponseTemplate::new(200).set_body_string(html))
            .mount(&server)
            .await;

        let m = GoPkgDev::with_base_url(Client::new(), server.uri())
            .search(&query())
            .await
            .expect("a bad date must not fail the source");
        assert_eq!(m.len(), 1, "the match itself must still come through");
        assert_eq!(m[0].last_updated, None);
    }
}

// ── progressive keyword narrowing, the ANDing registries ────────────────────
//
// Measured live against a 7-term idea: these sources return zero results for
// the full keyword set while 2 of its longest terms return plenty. Each test
// mocks the full query to empty and the narrowed one to real results, so it
// fails without the narrowing loop.

#[tokio::test]
async fn nuget_narrows_all_the_way_to_a_single_keyword() {
    let server = MockServer::start().await;
    for empty in [
        "ide code spell syntax mistakes",
        "spell syntax mistakes",
        "syntax mistakes",
    ] {
        Mock::given(method("GET"))
            .and(path("/query"))
            .and(query_param("q", empty))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "data": [] })))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/query"))
        .and(query_param("q", "mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(nuget_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = NuGet::with_search_url(Client::new(), server.uri())
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "Newtonsoft.Json");
}

#[tokio::test]
async fn crates_io_narrows_when_the_full_query_is_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .and(query_param("q", "ide code spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "crates": [] })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/crates"))
        .and(query_param("q", "spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(two_crate_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = source_for(&server)
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].name, "tokio");
}

#[tokio::test]
async fn rubygems_narrows_when_the_full_query_is_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search.json"))
        .and(query_param("query", "ide code spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/v1/search.json"))
        .and(query_param("query", "spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(rubygems_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = RubyGems::with_base_url(Client::new(), server.uri())
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "rails");
}

#[tokio::test]
async fn packagist_narrows_when_the_full_query_is_empty() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .and(query_param("q", "ide code spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "results": [] })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/search.json"))
        .and(query_param("q", "spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(packagist_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = packagist_for(&server)
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].name, "laravel/framework");
}

#[tokio::test]
async fn artifacthub_narrows_all_the_way_to_a_single_keyword() {
    let server = MockServer::start().await;
    for empty in [
        "ide code spell syntax mistakes",
        "spell syntax mistakes",
        "syntax mistakes",
    ] {
        Mock::given(method("GET"))
            .and(path("/api/v1/packages/search"))
            .and(query_param("ts_query_web", empty))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "packages": [] })))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/api/v1/packages/search"))
        .and(query_param("ts_query_web", "mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(artifacthub_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = artifacthub_for(&server)
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].name, "prometheus");
}

#[tokio::test]
async fn hacker_news_narrows_when_the_idea_and_full_query_are_empty() {
    // HN searches the raw idea first -- a phrase match is the best answer when
    // it lands -- then the keyword set, then the narrowings.
    let server = MockServer::start().await;
    for empty in [
        "an ide plugin that highlights spelling mistakes in code comments",
        "ide code spell syntax mistakes",
    ] {
        Mock::given(method("GET"))
            .and(path("/api/v1/search"))
            .and(query_param("query", empty))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "hits": [] })))
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/api/v1/search"))
        .and(query_param("query", "spell syntax mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(hn_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = hn_for(&server).search(&narrowing_query()).await.unwrap();
    assert_eq!(matches.len(), 2);
}

/// Maven is the strict one: its Solr index matches artifact coordinates only,
/// so even two terms come back empty and it has to fall all the way to one.
#[tokio::test]
async fn maven_narrows_all_the_way_to_a_single_keyword() {
    let server = MockServer::start().await;
    for empty in [
        "ide code spell syntax mistakes",
        "spell syntax mistakes",
        "syntax mistakes",
    ] {
        Mock::given(method("GET"))
            .and(path("/solrsearch/select"))
            .and(query_param("q", empty))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(json!({ "response": { "docs": [] } })),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/solrsearch/select"))
        .and(query_param("q", "mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(maven_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = Maven::with_base_url(Client::new(), server.uri())
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "com.google.guava:guava");
}

/// Homebrew ANDs locally against a one-line description, so the narrowing runs
/// over the cached catalog rather than over requests.
#[tokio::test]
async fn homebrew_narrows_against_the_cached_catalog() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/formula.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([
            {
                "name": "proselint",
                // Carries spell/syntax/mistakes but neither "ide" nor "code",
                // so only the narrowed keyword set can match it.
                "desc": "Finds syntax mistakes and spell errors in prose",
                "homepage": "https://proselint.com"
            }
        ])))
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/api/cask.json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&server)
        .await;

    let matches = Homebrew::with_base_url(Client::new(), server.uri())
        .search(&narrowing_query())
        .await
        .unwrap();
    assert_eq!(matches.len(), 1);
    assert_eq!(matches[0].name, "proselint");
}

/// The AUR matches the whole `arg` against one package, so like Maven and NuGet
/// it only recovers at a single keyword.
#[tokio::test]
async fn aur_narrows_all_the_way_to_a_single_keyword() {
    let server = MockServer::start().await;
    for empty in [
        "ide code spell syntax mistakes",
        "spell syntax mistakes",
        "syntax mistakes",
    ] {
        Mock::given(method("GET"))
            .and(path("/rpc/"))
            .and(query_param("arg", empty))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(json!({ "resultcount": 0, "results": [], "type": "search" })),
            )
            .expect(1)
            .mount(&server)
            .await;
    }
    Mock::given(method("GET"))
        .and(path("/rpc/"))
        .and(query_param("arg", "mistakes"))
        .respond_with(ResponseTemplate::new(200).set_body_json(aur_body()))
        .expect(1)
        .mount(&server)
        .await;

    let matches = aur_for(&server).search(&narrowing_query()).await.unwrap();
    assert_eq!(matches.len(), 2);
    assert_eq!(matches[0].name, "ccache");
}
