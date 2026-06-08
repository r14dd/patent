//! OpenAI-compatible chat-completions backend.
//!
//! Talks to any server that implements `POST {base}/chat/completions`: OpenAI,
//! OpenRouter, Groq, vLLM, LM Studio, llama.cpp, and others. The base URL is set
//! with `--api-base` (ending in `/v1`); auth, when the server needs it, comes
//! from `--api-key` or the `OPENAI_API_KEY` environment variable.

use crate::llm::Llm;

/// Client for an OpenAI-compatible chat endpoint.
pub struct OpenAi {
    base: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl OpenAi {
    pub fn new(base: impl Into<String>, model: impl Into<String>, api_key: Option<String>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("failed to build HTTP client");
        Self {
            base: base.into(),
            model: model.into(),
            api_key,
            client,
        }
    }
}

#[async_trait::async_trait]
impl Llm for OpenAi {
    async fn generate(&self, prompt: &str) -> crate::Result<String> {
        let url = format!("{}/chat/completions", self.base.trim_end_matches('/'));
        let body = serde_json::json!({
            "model": self.model,
            "messages": [{ "role": "user", "content": prompt }],
            "temperature": 0.0,
            "max_tokens": 512,
        });

        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }

        let response = req.send().await.map_err(|e| {
            crate::Error::LlmUnreachable(format!(
                "OpenAI-compatible API at {} not reachable ({e}). Check --api-base.",
                self.base
            ))
        })?;

        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|e| crate::Error::Parse(e.to_string()))?;

        // A non-2xx is recoverable (bad model, missing/invalid key, server down):
        // surface the API's error message so the run degrades to a search-only
        // result instead of aborting.
        if !status.is_success() {
            let reason = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["error"]["message"].as_str().map(String::from))
                .unwrap_or_else(|| format!("HTTP {}", status.as_u16()));
            let msg = format!(
                "{reason} (model `{}`). Check --model and --api-key.",
                self.model
            );
            return Err(if status.is_server_error() {
                crate::Error::LlmUnreachable(msg)
            } else {
                crate::Error::LlmRejected(msg)
            });
        }

        // A 200 with an unexpected body (non-JSON proxy page, empty choices, null
        // content) is the server misbehaving, not our bug: treat it as LlmRejected
        // so the run still degrades to a search-only result.
        let json: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
            crate::Error::LlmRejected(format!(
                "API at {} returned a non-JSON response (model `{}`).",
                self.base, self.model
            ))
        })?;

        json["choices"][0]["message"]["content"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| {
                crate::Error::LlmRejected(format!(
                    "API returned no message content (model `{}`).",
                    self.model
                ))
            })
    }

    fn label(&self) -> &str {
        "OpenAI API"
    }
}
