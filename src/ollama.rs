//! Minimal Ollama client (`localhost:11434/api/generate`).
//!
//! Returns a clear, actionable error when the daemon is unreachable.

use crate::llm::Llm;

/// Default Ollama endpoint.
pub const DEFAULT_ENDPOINT: &str = "http://localhost:11434";

/// Default generation model. `qwen3.5` (9B) for quality; `qwen2.5:3b` if RAM is
/// tight. Override via `--model`.
pub const DEFAULT_MODEL: &str = "qwen3.5";

/// A thin handle over the Ollama generate API.
#[derive(Debug, Clone)]
pub struct Ollama {
    endpoint: String,
    model: String,
    client: reqwest::Client,
}

impl Ollama {
    pub fn new(endpoint: impl Into<String>, model: impl Into<String>) -> crate::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(crate::Error::HttpClient)?;
        Ok(Self {
            endpoint: endpoint.into(),
            model: model.into(),
            client,
        })
    }
}

#[async_trait::async_trait]
impl Llm for Ollama {
    /// Send a prompt to `/api/generate` and return the completion text.
    async fn generate(&self, prompt: &str) -> crate::Result<String> {
        let url = format!("{}/api/generate", self.endpoint);
        let body = serde_json::json!({
            "model": self.model,
            "prompt": prompt,
            "stream": false,
            // Thinking models (qwen3.5 and later) otherwise spend the whole
            // `num_predict` budget on a reasoning trace that Ollama returns in
            // a separate `thinking` field, leaving `response` empty -- every
            // verdict would silently fall back. Measured: 512 tokens of
            // thinking and no answer, vs 11 tokens with thinking off. The
            // verdict is a small fixed JSON object, so the trace buys nothing.
            // Accepted (HTTP 200, ignored) by non-thinking models like qwen2.5.
            "think": false,
            "options": {
                "temperature": 0.0,
                "num_predict": 512,
            },
        });

        let response = self
            .client
            .post(&url)
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                crate::Error::LlmUnreachable(format!(
                    "Ollama not reachable at {} ({e}). Run `ollama serve` and `ollama pull {}`.",
                    self.endpoint, self.model
                ))
            })?;

        let status = response.status();
        let body = response
            .text()
            .await
            .map_err(|e| crate::Error::Parse(e.to_string()))?;

        // A non-success status (most commonly 404 for a model that hasn't been
        // pulled, or a 5xx from a proxy in front of Ollama) is a recoverable
        // error: surface it as LlmRejected so the caller still renders results
        // instead of aborting. We try to lift Ollama's `{"error": "..."}`
        // message but never depend on the body being JSON.
        if !status.is_success() {
            let reason = serde_json::from_str::<serde_json::Value>(&body)
                .ok()
                .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(String::from))
                .unwrap_or_else(|| format!("Ollama returned HTTP {}", status.as_u16()));
            let msg = format!(
                "{reason} (model `{}`). Run `ollama pull {}`.",
                self.model, self.model
            );
            return Err(if status.is_server_error() {
                crate::Error::LlmUnreachable(msg)
            } else {
                crate::Error::LlmRejected(msg)
            });
        }

        let json: serde_json::Value =
            serde_json::from_str(&body).map_err(|e| crate::Error::Parse(e.to_string()))?;

        // Some Ollama builds return 200 with an `{"error": ...}` body instead.
        if let Some(err) = json.get("error").and_then(|e| e.as_str()) {
            return Err(crate::Error::LlmRejected(format!(
                "{err} (model `{}`). Run `ollama pull {}`.",
                self.model, self.model
            )));
        }

        json["response"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| crate::Error::Parse("missing 'response' field".into()))
    }

    fn label(&self) -> &str {
        "Ollama"
    }
}
