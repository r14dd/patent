use patent::model::{Match, Query, Saturation, Source};
use patent::ollama::Ollama;
use patent::verdict::{self, CAVEAT};
use serde_json::json;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn query() -> Query {
    Query {
        idea: "a cli to kill processes on a port".to_string(),
        keywords: vec!["kill".into(), "port".into()],
    }
}

fn checked() -> Vec<Source> {
    vec![Source::Npm, Source::CratesIo]
}

fn sample_matches() -> Vec<Match> {
    vec![
        Match {
            name: "kill-port".to_string(),
            source: Source::Npm,
            url: "https://npmjs.com/package/kill-port".to_string(),
            description: "Kill process on a port".to_string(),
            popularity: Some(50_000),
            similarity: 0.85,
            last_updated: None,
        },
        Match {
            name: "fkill-cli".to_string(),
            source: Source::Npm,
            url: "https://npmjs.com/package/fkill-cli".to_string(),
            description: "Fabulously kill processes".to_string(),
            popularity: Some(10_000),
            similarity: 0.60,
            last_updated: None,
        },
    ]
}

// -- build_prompt tests -------------------------------------------------------

#[test]
fn prompt_contains_the_idea() {
    let prompt = verdict::build_prompt(&query(), &sample_matches(), &checked());
    assert!(
        prompt.contains("a cli to kill processes on a port"),
        "prompt must include the user's idea"
    );
}

#[test]
fn prompt_contains_match_names() {
    let prompt = verdict::build_prompt(&query(), &sample_matches(), &checked());
    assert!(prompt.contains("kill-port"));
    assert!(prompt.contains("fkill-cli"));
}

#[test]
fn prompt_includes_popularity_and_urls() {
    // #60: the model should see how popular/real each match is and where it lives,
    // so it can weight firmly-established prior art.
    let prompt = verdict::build_prompt(&query(), &sample_matches(), &checked());
    assert!(
        prompt.contains("popularity 50000"),
        "prompt must include the match popularity figure"
    );
    assert!(
        prompt.contains("https://npmjs.com/package/kill-port"),
        "prompt must include the match URL"
    );
}

#[test]
fn prompt_dates_the_matches_whose_source_publishes_one() {
    // #29: the model weighs prior art better when it can see how live each
    // match is — and an undated match must read as "no date published", never
    // as a match that happens to be old.
    let mut matches = sample_matches();
    matches[0].last_updated = Some("2015-03-01T00:00:00Z".to_string());
    let prompt = verdict::build_prompt(&query(), &matches, &checked());

    let dated = prompt
        .lines()
        .find(|l| l.contains("kill-port"))
        .expect("dated match must appear in the prompt");
    assert!(
        dated.contains("updated") && dated.contains("years ago"),
        "a dated match must carry its age: {dated}"
    );

    let undated = prompt
        .lines()
        .find(|l| l.contains("fkill-cli"))
        .expect("undated match must appear in the prompt");
    assert!(
        !undated.contains("updated"),
        "an undated match must claim nothing about its age: {undated}"
    );
}

#[test]
fn prompt_warns_that_age_is_not_a_reason_to_discount_prior_art() {
    // Integrity: staleness is colour on the prior art, not a licence to call a
    // crowded space open. If the guidance ever drops out of the prompt, a model
    // is free to reason "it's old, therefore it doesn't count".
    let prompt = verdict::build_prompt(&query(), &sample_matches(), &checked());
    let lower = prompt.to_lowercase();
    assert!(
        lower.contains("never as a reason to discount"),
        "prompt must tell the model not to discount prior art for being stale"
    );
    assert!(
        lower.contains("not evidence of staleness"),
        "prompt must explain that an absent date is not a staleness signal"
    );
}

#[test]
fn prompt_forbids_asserting_absence() {
    let prompt = verdict::build_prompt(&query(), &sample_matches(), &checked());
    let lower = prompt.to_lowercase();
    assert!(
        lower.contains("never") || lower.contains("do not") || lower.contains("must not"),
        "prompt must forbid claiming non-existence"
    );
}

#[test]
fn prompt_requires_json_output() {
    let prompt = verdict::build_prompt(&query(), &sample_matches(), &checked());
    let lower = prompt.to_lowercase();
    assert!(lower.contains("json"), "prompt must ask for JSON output");
}

#[test]
fn prompt_with_no_matches_still_valid() {
    let prompt = verdict::build_prompt(&query(), &[], &checked());
    assert!(!prompt.is_empty());
    assert!(prompt.contains("a cli to kill processes on a port"));
}

#[test]
fn prompt_names_only_the_sources_actually_checked() {
    // Integrity: the model must only be told about coverage that really
    // happened. With HN not in the reached set, it must not be named.
    let prompt = verdict::build_prompt(&query(), &sample_matches(), &checked());
    assert!(prompt.contains("npm"), "must name a reached source");
    assert!(prompt.contains("crates.io"), "must name a reached source");
    assert!(
        !prompt.contains("Hacker News"),
        "must not name a source that wasn't reached"
    );
}

#[test]
fn prompt_uses_skeptical_reviewer_persona() {
    let prompt = verdict::build_prompt(&query(), &sample_matches(), &checked());
    let lower = prompt.to_lowercase();
    assert!(
        lower.contains("skeptic") || lower.contains("default assumption"),
        "prompt should frame the reviewer as skeptical"
    );
}

#[tokio::test]
async fn assess_drops_gaps_naming_a_top_match() {
    // A gap that names a top-10 match by name should be filtered out — the model
    // naming "kill-port" in a gap is confirming it exists, not identifying open
    // space. A clean gap with no match name must survive.
    let server = mock_ollama(ollama_response(
        "Crowded",
        "Several tools exist in the sources checked.",
        &[
            "kill-port does not support IPv6",
            "none of the tools support Windows arm64",
        ],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &sample_matches(), checked(), vec![])
        .await
        .unwrap();

    assert_eq!(v.gaps.len(), 1, "gap naming a top match must be dropped");
    assert!(
        v.gaps[0].contains("Windows arm64"),
        "clean gap should survive"
    );
}

#[tokio::test]
async fn assess_gap_filter_uses_word_boundaries() {
    // "at" is a match name that appears as a *substring* inside "automatically".
    // The gap must survive — only whole-word occurrences should be filtered.
    let matches_with_short_name = vec![Match {
        name: "at".to_string(),
        source: Source::Npm,
        url: "https://npmjs.com/package/at".into(),
        description: "schedule tasks".into(),
        popularity: Some(1000),
        similarity: 0.80,
        last_updated: None,
    }];

    let server = mock_ollama(ollama_response(
        "Crowded",
        "Several tools exist in the sources checked.",
        &[
            "automatically handles port conflicts", // "at" is a substring here → must survive
            "at startup it reads config",           // "at" as a whole word → must be dropped
        ],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(
        &ollama,
        &query(),
        &matches_with_short_name,
        checked(),
        vec![],
    )
    .await
    .unwrap();

    assert_eq!(
        v.gaps.len(),
        1,
        "only the whole-word 'at' gap should be dropped"
    );
    assert!(
        v.gaps[0].contains("automatically"),
        "substring occurrence must not filter the gap"
    );
}

// -- assess() end-to-end via wiremock -----------------------------------------

fn ollama_response(level: &str, headline: &str, gaps: &[&str]) -> serde_json::Value {
    let model_json = json!({
        "level": level,
        "headline": headline,
        "gaps": gaps,
    });
    json!({
        "response": model_json.to_string(),
        "done": true,
    })
}

async fn mock_ollama(response: serde_json::Value) -> MockServer {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(response))
        .mount(&server)
        .await;
    server
}

#[tokio::test]
async fn assess_returns_verdict_with_caveat() {
    let server = mock_ollama(ollama_response(
        "Saturated",
        "Lots of prior art found in the sources checked.",
        &["no Windows support in existing tools"],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let sources = checked();
    let v = verdict::assess(
        &ollama,
        &query(),
        &sample_matches(),
        sources.clone(),
        vec![],
    )
    .await
    .unwrap();

    assert_eq!(v.level, Saturation::Saturated);
    assert!(v.headline.contains("prior art"));
    assert_eq!(v.gaps.len(), 1);
    assert_eq!(v.sources_checked, sources);
    assert_eq!(v.caveat, CAVEAT);
}

#[tokio::test]
async fn assess_parses_open_level() {
    let server = mock_ollama(ollama_response(
        "Open",
        "Nothing close found in the sources checked.",
        &[],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &[], vec![Source::GitHub], vec![])
        .await
        .unwrap();

    assert_eq!(v.level, Saturation::Open);
}

#[tokio::test]
async fn assess_parses_crowded_level() {
    let server = mock_ollama(ollama_response(
        "Crowded",
        "A few adjacent tools exist.",
        &["gap one", "gap two"],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(
        &ollama,
        &query(),
        &sample_matches(),
        vec![Source::Npm],
        vec![],
    )
    .await
    .unwrap();

    assert_eq!(v.level, Saturation::Crowded);
    assert_eq!(v.gaps.len(), 2);
}

#[tokio::test]
async fn assess_handles_json_wrapped_in_markdown_fence() {
    let fenced = "```json\n{\"level\":\"Open\",\"headline\":\"Nothing found.\",\"gaps\":[]}\n```";
    let server = mock_ollama(json!({"response": fenced, "done": true})).await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &[], vec![Source::GitHub], vec![])
        .await
        .unwrap();
    assert_eq!(v.level, Saturation::Open);
}

#[tokio::test]
async fn assess_rejects_garbage_response() {
    let server = mock_ollama(json!({"response": "I don't know what JSON is", "done": true})).await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let err = verdict::assess(&ollama, &query(), &[], vec![], vec![])
        .await
        .unwrap_err();
    assert!(matches!(err, patent::Error::Parse(_)));
}

#[tokio::test]
async fn assess_floors_level_when_model_underrates_a_crowded_space() {
    // The model says "Open" but the similarity data shows two close matches
    // (0.85, 0.60). The level must be floored up to Crowded, and because the
    // model misjudged, its headline is replaced with a safe data-derived one.
    let server = mock_ollama(ollama_response(
        "Open",
        "This is a brand-new idea about a testing framework.",
        &[],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &sample_matches(), checked(), vec![])
        .await
        .unwrap();

    assert_eq!(
        v.level,
        Saturation::Crowded,
        "two >=0.55 matches => Crowded"
    );
    assert!(
        !v.headline.contains("testing framework"),
        "a floored level must not keep the model's misjudged headline"
    );
    assert!(v.headline.to_lowercase().contains("sources checked"));
}

#[tokio::test]
async fn assess_replaces_absence_claiming_headline() {
    // No matches => level stays Open, so the model's headline is kept *unless*
    // it asserts non-existence — which it does here, and must be replaced.
    let server = mock_ollama(ollama_response(
        "Open",
        "This tool does not exist and there is no prior art for it.",
        &[],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &[], vec![Source::GitHub], vec![])
        .await
        .unwrap();

    let lower = v.headline.to_lowercase();
    assert!(
        !lower.contains("does not exist"),
        "absence claim must be scrubbed"
    );
    assert!(
        !lower.contains("no prior art"),
        "absence claim must be scrubbed"
    );
    assert_eq!(v.caveat, CAVEAT);
}

#[tokio::test]
async fn assess_drops_gaps_that_assert_absence() {
    // A gap bullet that smuggles an absence claim must be filtered out; a
    // legitimate gap is kept.
    let server = mock_ollama(ollama_response(
        "Open",
        "A few adjacent tools turned up in the sources checked.",
        &[
            "no existing tool supports Windows, and there is no prior art for this",
            "none of the matches offer an async API",
        ],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &[], vec![Source::GitHub], vec![])
        .await
        .unwrap();

    assert_eq!(v.gaps.len(), 1, "the absence-asserting gap must be dropped");
    assert!(v.gaps[0].contains("async API"));
    for g in &v.gaps {
        assert!(!g.to_lowercase().contains("no prior art"));
        assert!(!g.to_lowercase().contains("no existing tool"));
    }
}

#[tokio::test]
async fn assess_scrubs_broadened_absence_phrasings() {
    // Phrasings beyond the obvious ones must also be caught.
    for headline in [
        "This has not been implemented anywhere yet.",
        "This is unprecedented — no similar tool exists.",
        "There is no existing software like this.",
    ] {
        let server = mock_ollama(ollama_response("Open", headline, &[])).await;
        let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
        let v = verdict::assess(&ollama, &query(), &[], vec![Source::GitHub], vec![])
            .await
            .unwrap();
        let lower = v.headline.to_lowercase();
        assert!(
            !lower.contains("has not been implemented")
                && !lower.contains("unprecedented")
                && !lower.contains("no similar tool")
                && !lower.contains("no existing software"),
            "absence headline survived: {:?}",
            v.headline
        );
    }
}

#[tokio::test]
async fn assess_scrubs_absence_claims_dressed_up_as_maintenance() {
    // #29 opened a second door to the one claim this tool must never make.
    // With dates in the prompt, "nothing exists" becomes "nothing *maintained*
    // exists" — which is just as unprovable (only 9 of 19 sources publish a
    // date, so an undated match is not an unmaintained one) and reads just as
    // much like a green light. `sample_matches` floors to Crowded, which is
    // what the model returns here, so the headline survives the floor and it is
    // genuinely `guard_headline` doing the scrubbing.
    for headline in [
        "No actively maintained tool solves this in the sources checked.",
        "No maintained alternative turned up.",
        "Nothing maintained covers this idea.",
        "There is no currently maintained tool for this.",
        "No actively developed option covers this.",
        "No up-to-date implementation of this exists.",
    ] {
        let server = mock_ollama(ollama_response("Crowded", headline, &[])).await;
        let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
        let v = verdict::assess(&ollama, &query(), &sample_matches(), checked(), vec![])
            .await
            .unwrap();

        assert_ne!(
            v.headline, headline,
            "maintenance-flavoured absence claim survived: {:?}",
            v.headline
        );
        assert!(
            v.headline.to_lowercase().contains("sources checked"),
            "the replacement must be the scoped data headline, got {:?}",
            v.headline
        );
    }
}

#[tokio::test]
async fn assess_drops_gaps_claiming_nothing_maintained_exists() {
    let server = mock_ollama(ollama_response(
        "Crowded",
        "Several tools turned up in the sources checked.",
        &[
            "no actively maintained tool offers a TUI",
            "none of the matches offer an async API",
        ],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &sample_matches(), checked(), vec![])
        .await
        .unwrap();

    assert_eq!(
        v.gaps.len(),
        1,
        "the gap asserting nothing maintained exists must be dropped, got {:?}",
        v.gaps
    );
    assert!(v.gaps[0].contains("async API"));
}

#[tokio::test]
async fn assess_keeps_an_honest_note_that_a_match_is_unmaintained() {
    // The other side of the guard above, and the whole point of the feature:
    // saying a *specific* match looks abandoned is read straight off the data
    // we showed the user. Over-scrubbing that would throw away the signal #29
    // exists to surface, so this pins the boundary.
    let honest = "Crowded — fkill-cli covers this, though it is no longer maintained.";
    let server = mock_ollama(ollama_response("Crowded", honest, &[])).await;
    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &sample_matches(), checked(), vec![])
        .await
        .unwrap();

    assert_eq!(
        v.headline, honest,
        "a per-match maintenance note is colour, not an absence claim"
    );
}

#[tokio::test]
async fn assess_threads_failed_sources_into_verdict() {
    let server = mock_ollama(ollama_response("Open", "Nothing close turned up.", &[])).await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(
        &ollama,
        &query(),
        &[],
        vec![Source::GitHub],
        vec![Source::PyPI, Source::CratesIo],
    )
    .await
    .unwrap();

    assert_eq!(v.sources_failed, vec![Source::PyPI, Source::CratesIo]);
}

#[tokio::test]
async fn assess_replaces_no_match_headline_when_a_close_match_exists() {
    // A single 0.57 match keeps the level Open, but a "no direct matches" headline
    // would be misleading when that match is real prior art. It must be replaced
    // with one that names the close match.
    let matches = vec![Match {
        name: "patent".into(),
        source: Source::CratesIo,
        url: "https://crates.io/crates/patent".into(),
        description: "A prior-art search for your code ideas".into(),
        popularity: None,
        similarity: 0.57,
        last_updated: None,
    }];
    let server = mock_ollama(ollama_response(
        "Open",
        "No direct matches found in the sources checked.",
        &[],
    ))
    .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &matches, checked(), vec![])
        .await
        .unwrap();

    assert_eq!(v.level, Saturation::Open, "one 0.57 match stays Open");
    let lower = v.headline.to_lowercase();
    assert!(
        !lower.contains("no direct match"),
        "misleading 'no match' headline must be replaced: {:?}",
        v.headline
    );
    assert!(
        lower.contains("closely-related"),
        "replacement headline should name the close match: {:?}",
        v.headline
    );
}

#[tokio::test]
async fn assess_floors_single_very_strong_match_to_crowded() {
    // A lone near-identical match (>= 0.70) must lift the level off Open even
    // though only one match exists (the close >= 2 path does not apply here).
    let matches = vec![Match {
        name: "twin".into(),
        source: Source::CratesIo,
        url: "https://crates.io/crates/twin".into(),
        description: "Nearly the same idea".into(),
        popularity: None,
        similarity: 0.72,
        last_updated: None,
    }];
    let server = mock_ollama(ollama_response("Open", "A benign headline.", &[])).await;
    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &matches, checked(), vec![])
        .await
        .unwrap();
    assert_eq!(v.level, Saturation::Crowded, "0.72 single match => Crowded");
    assert!(v.headline.to_lowercase().contains("closely-related"));
}

#[tokio::test]
async fn assess_retries_on_server_error() {
    let server = MockServer::start().await;

    // 500 registered first — expires after one match, then the 200 takes over.
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(ResponseTemplate::new(500))
        .up_to_n_times(1)
        .mount(&server)
        .await;

    // 200 registered second — becomes active once the 500 is exhausted.
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(ollama_response(
            "Crowded",
            "Several tools exist in the sources checked.",
            &[],
        )))
        .mount(&server)
        .await;

    let ollama = Ollama::new(server.uri(), "qwen2.5").unwrap();
    let v = verdict::assess(&ollama, &query(), &sample_matches(), checked(), vec![])
        .await
        .unwrap();

    assert_eq!(v.level, Saturation::Crowded);
}

// -- from_data() — the --fast / no-LLM path -----------------------------------

#[test]
fn from_data_floors_level_against_similarity() {
    // Two matches at 0.85 and 0.60 => at least two >= 0.55 => Crowded, derived
    // from the similarity data alone with no model in the loop. The no-LLM path
    // must never under-rate a populated space into a green "Open".
    let v = verdict::from_data(&sample_matches(), checked(), vec![]);
    assert_eq!(v.level, Saturation::Crowded);
    assert!(v.gaps.is_empty(), "no model => no gaps");
    assert_eq!(v.caveat, CAVEAT);
    assert_eq!(v.sources_checked, checked());
}

#[test]
fn from_data_open_when_nothing_close() {
    let v = verdict::from_data(&[], vec![Source::GitHub], vec![]);
    assert_eq!(v.level, Saturation::Open);
    assert_eq!(v.caveat, CAVEAT);
}

#[test]
fn from_data_floors_single_very_strong_match_to_crowded() {
    // The no-LLM path routes through the same floor: one >= 0.70 match => Crowded.
    let matches = vec![Match {
        name: "twin".into(),
        source: Source::CratesIo,
        url: "https://crates.io/crates/twin".into(),
        description: "Nearly the same idea".into(),
        popularity: None,
        similarity: 0.72,
        last_updated: None,
    }];
    let v = verdict::from_data(&matches, checked(), vec![]);
    assert_eq!(v.level, Saturation::Crowded);
    assert!(v.gaps.is_empty());
    assert_eq!(v.caveat, CAVEAT);
}

#[test]
fn from_data_never_asserts_absence() {
    // The integrity rule holds on the no-LLM path too: even with zero matches
    // the headline must never claim the idea doesn't exist anywhere.
    let v = verdict::from_data(&[], vec![Source::GitHub], vec![]);
    let lower = v.headline.to_lowercase();
    for phrase in [
        "does not exist",
        "no prior art",
        "never been",
        "unprecedented",
    ] {
        assert!(
            !lower.contains(phrase),
            "absence claim in fast headline: {:?}",
            v.headline
        );
    }
}

#[test]
fn from_data_threads_failed_sources() {
    let v = verdict::from_data(&sample_matches(), checked(), vec![Source::PyPI]);
    assert_eq!(v.sources_failed, vec![Source::PyPI]);
}
