use patent::llm::Llm;
use patent::ollama::Ollama;
use serde_json::json;
use wiremock::matchers::{body_json_string, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

fn ollama_for(server: &MockServer, model: &str) -> Ollama {
    Ollama::new(server.uri(), model).unwrap()
}

fn generate_response(text: &str) -> serde_json::Value {
    json!({ "response": text, "done": true })
}

#[tokio::test]
async fn generate_returns_response_text() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(ResponseTemplate::new(200).set_body_json(generate_response("hello world")))
        .mount(&server)
        .await;

    let result = ollama_for(&server, "qwen2.5")
        .generate("say hi")
        .await
        .unwrap();
    assert_eq!(result, "hello world");
}

#[tokio::test]
async fn generate_sends_model_and_prompt() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .and(body_json_string(
            json!({
                "model": "qwen2.5",
                "prompt": "say hi",
                "stream": false,
                "options": {
                    "temperature": 0.0,
                    "num_predict": 512,
                }
            })
            .to_string(),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(generate_response("ok")))
        .expect(1)
        .mount(&server)
        .await;

    ollama_for(&server, "qwen2.5")
        .generate("say hi")
        .await
        .unwrap();
}

#[tokio::test]
async fn generate_maps_connection_error_to_llm_unreachable() {
    let ollama = Ollama::new("http://127.0.0.1:1", "qwen2.5").unwrap();
    let err = ollama.generate("hi").await.unwrap_err();
    assert!(
        matches!(err, patent::Error::LlmUnreachable(_)),
        "expected LlmUnreachable, got: {err:?}"
    );
}

#[tokio::test]
async fn generate_maps_server_error_to_parse() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(ResponseTemplate::new(200).set_body_string("not json"))
        .mount(&server)
        .await;

    let err = ollama_for(&server, "qwen2.5")
        .generate("hi")
        .await
        .unwrap_err();
    assert!(
        matches!(err, patent::Error::Parse(_)),
        "expected Parse, got: {err:?}"
    );
}

#[tokio::test]
async fn generate_maps_model_not_found_to_llm_rejected() {
    // Ollama is reachable but the model isn't pulled: it returns 404 with an
    // {"error": ...} body. This must be a recoverable LlmRejected error (so the
    // run degrades gracefully), not a fatal Parse error.
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/api/generate"))
        .respond_with(
            ResponseTemplate::new(404).set_body_json(
                json!({ "error": "model 'qwen2.5' not found, try pulling it first" }),
            ),
        )
        .mount(&server)
        .await;

    let err = ollama_for(&server, "qwen2.5")
        .generate("hi")
        .await
        .unwrap_err();
    assert!(
        matches!(err, patent::Error::LlmRejected(_)),
        "expected LlmRejected, got: {err:?}"
    );
}
