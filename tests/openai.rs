use patent::llm::Llm;
use patent::openai::OpenAi;
use serde_json::json;
use wiremock::matchers::{body_json_string, header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn chat_response(content: &str) -> serde_json::Value {
    json!({ "choices": [{ "message": { "role": "assistant", "content": content } }] })
}

#[tokio::test]
async fn generate_returns_message_content() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("hi there")))
        .mount(&server)
        .await;

    let client = OpenAi::new(server.uri(), "gpt-4o-mini", None).unwrap();
    assert_eq!(client.generate("say hi").await.unwrap(), "hi there");
}

#[tokio::test]
async fn generate_sends_model_and_user_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json_string(
            json!({
                "model": "gpt-4o-mini",
                "messages": [{ "role": "user", "content": "say hi" }],
                "temperature": 0.0,
                "max_tokens": 2048
            })
            .to_string(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("ok")))
        .expect(1)
        .mount(&server)
        .await;

    OpenAi::new(server.uri(), "gpt-4o-mini", None)
        .unwrap()
        .generate("say hi")
        .await
        .unwrap();
}

#[tokio::test]
async fn generate_sends_bearer_auth_when_key_set() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(header("authorization", "Bearer sk-test"))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("ok")))
        .expect(1)
        .mount(&server)
        .await;

    OpenAi::new(server.uri(), "gpt-4o-mini", Some("sk-test".into()))
        .unwrap()
        .generate("hi")
        .await
        .unwrap();
}

#[tokio::test]
async fn generate_maps_connection_error_to_llm_unreachable() {
    let client = OpenAi::new("http://127.0.0.1:1", "gpt-4o-mini", None).unwrap();
    let err = client.generate("hi").await.unwrap_err();
    assert!(
        matches!(err, patent::Error::LlmUnreachable(_)),
        "expected LlmUnreachable, got: {err:?}"
    );
}

#[tokio::test]
async fn generate_maps_http_error_to_llm_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(401)
                .set_body_json(json!({ "error": { "message": "Incorrect API key provided" } })),
        )
        .mount(&server)
        .await;

    let err = OpenAi::new(server.uri(), "gpt-4o-mini", None)
        .unwrap()
        .generate("hi")
        .await
        .unwrap_err();
    let patent::Error::LlmRejected(msg) = err else {
        panic!("expected LlmRejected, got: {err:?}");
    };
    assert!(
        msg.contains("Incorrect API key provided"),
        "should surface the API error message, got: {msg}"
    );
}

#[tokio::test]
async fn generate_maps_empty_choices_to_llm_rejected() {
    // A 200 with no usable content must degrade (recoverable), not abort the run.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({ "choices": [] })))
        .mount(&server)
        .await;

    let err = OpenAi::new(server.uri(), "gpt-4o-mini", None)
        .unwrap()
        .generate("hi")
        .await
        .unwrap_err();
    assert!(
        matches!(err, patent::Error::LlmRejected(_)),
        "expected LlmRejected, got: {err:?}"
    );
}

#[tokio::test]
async fn generate_maps_non_json_success_body_to_llm_rejected() {
    // A proxy returning a 200 HTML page must degrade, not abort.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(ResponseTemplate::new(200).set_body_string("<html>proxy</html>"))
        .mount(&server)
        .await;

    let err = OpenAi::new(server.uri(), "gpt-4o-mini", None)
        .unwrap()
        .generate("hi")
        .await
        .unwrap_err();
    assert!(
        matches!(err, patent::Error::LlmRejected(_)),
        "expected LlmRejected, got: {err:?}"
    );
}

#[tokio::test]
async fn generate_retries_reasoning_models_with_max_completion_tokens() {
    // OpenAI's o-series and GPT-5 reject `max_tokens` and a non-default
    // `temperature`. The retry must drop both and use the newer spelling.
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json_string(
            json!({
                "model": "o3",
                "messages": [{ "role": "user", "content": "say hi" }],
                "temperature": 0.0,
                "max_tokens": 2048
            })
            .to_string(),
        ))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "message": "Unsupported parameter: 'max_tokens' is not supported with \
                            this model. Use 'max_completion_tokens' instead."
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json_string(
            json!({
                "model": "o3",
                "messages": [{ "role": "user", "content": "say hi" }],
                "max_completion_tokens": 2048
            })
            .to_string(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("reasoned")))
        .expect(1)
        .mount(&server)
        .await;

    let out = OpenAi::new(server.uri(), "o3", None)
        .unwrap()
        .generate("say hi")
        .await
        .unwrap();
    assert_eq!(out, "reasoned");
}

#[tokio::test]
async fn generate_retries_when_the_server_rejects_only_temperature() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json_string(
            json!({
                "model": "o3",
                "messages": [{ "role": "user", "content": "say hi" }],
                "temperature": 0.0,
                "max_tokens": 2048
            })
            .to_string(),
        ))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": {
                "message": "Unsupported value: 'temperature' does not support 0.0 with \
                            this model. Only the default (1) value is supported."
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .and(body_json_string(
            json!({
                "model": "o3",
                "messages": [{ "role": "user", "content": "say hi" }],
                "max_completion_tokens": 2048
            })
            .to_string(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(chat_response("reasoned")))
        .expect(1)
        .mount(&server)
        .await;

    let out = OpenAi::new(server.uri(), "o3", None)
        .unwrap()
        .generate("say hi")
        .await
        .unwrap();
    assert_eq!(out, "reasoned");
}

#[tokio::test]
async fn generate_does_not_retry_an_unrelated_bad_request() {
    // An ordinary 400 (bad model name, malformed request) must degrade on the
    // first response -- the retry is for one documented incompatibility only.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(400).set_body_json(
                json!({ "error": { "message": "The model `nope` does not exist" } }),
            ),
        )
        .expect(1)
        .mount(&server)
        .await;

    let err = OpenAi::new(server.uri(), "nope", None)
        .unwrap()
        .generate("hi")
        .await
        .unwrap_err();
    let patent::Error::LlmRejected(msg) = err else {
        panic!("expected LlmRejected, got: {err:?}");
    };
    assert!(msg.contains("does not exist"), "got: {msg}");
}
