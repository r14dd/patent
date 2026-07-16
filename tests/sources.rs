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
use patent::sources::maven::Maven;
use patent::sources::npm::Npm;
use patent::sources::nuget::NuGet;
use patent::sources::packagist::Packagist;
use patent::sources::pypi::PyPI;
use patent::sources::rubygems::RubyGems;
use patent::sources::vscode::VsCodeMarketplace;
use patent::sources::{dedup, search_sources, SearchOutcome, SourceAdapter};
use reqwest::Client;
use serde_json::json;
use wiremock::matchers::{header_exists, method, path, query_param, query_param_is_missing};
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
            { "package": { "name": "express", "description": "Fast web framework." } },
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

// ---------------------------------------------------------------------------
// Go (pkg.go.dev HTML scrape)
// ---------------------------------------------------------------------------

fn go_html() -> String {
    r#"<!doctype html><html><body>
      <div class="SearchSnippet">
        <a href="/github.com/spf13/cobra">cobra</a>
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

// ---------------------------------------------------------------------------
// Maven Central (Solr JSON)
// ---------------------------------------------------------------------------

fn maven_body() -> serde_json::Value {
    json!({
        "response": {
            "docs": [
                { "g": "com.google.guava", "a": "guava", "versionCount": 50 }
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
                "statistics": [{ "statisticName": "install", "value": 12345.0 }]
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
async fn hex_sends_joined_keywords_as_search_param() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/api/packages"))
        .and(query_param("search", "async runtime"))
        .respond_with(ResponseTemplate::new(200).set_body_json(hex_body()))
        .expect(1)
        .mount(&server)
        .await;

    assert_eq!(hex_for(&server).search(&query()).await.unwrap().len(), 2);
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
