//! OpenAI-compatible chat-completions backend.
//!
//! Talks to any server that implements `POST {base}/chat/completions`: OpenAI,
//! OpenRouter, Groq, vLLM, LM Studio, llama.cpp, and others. The base URL is set
//! with `--api-base` (ending in `/v1`); auth, when the server needs it, comes
//! from `--api-key` or the `OPENAI_API_KEY` environment variable.

use crate::llm::Llm;

/// Does this 400 say the server refused `max_tokens` or `temperature`?
///
/// Matched against the error text rather than the model name because proxies
/// rename models freely (`openai/gpt-5`, `azure/o3`, a local alias), so the
/// server's own complaint is the only reliable signal.
fn rejects_our_parameters(body: &str) -> bool {
    let msg = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"]["message"].as_str().map(String::from))
        .unwrap_or_else(|| body.to_string())
        .to_lowercase();

    msg.contains("max_completion_tokens")
        || (msg.contains("temperature") && msg.contains("unsupported"))
}

/// Client for an OpenAI-compatible chat endpoint.
#[derive(Debug, Clone)]
pub struct OpenAi {
    base: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::Client,
}

impl OpenAi {
    /// Send one chat-completions request, returning the status and raw body.
    async fn post(&self, body: &serde_json::Value) -> crate::Result<(reqwest::StatusCode, String)> {
        let url = format!("{}/chat/completions", self.base.trim_end_matches('/'));
        let mut req = self.client.post(&url).json(body);
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
        Ok((status, text))
    }

    pub fn new(
        base: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> crate::Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .map_err(crate::Error::HttpClient)?;
        Ok(Self {
            base: base.into(),
            model: model.into(),
            api_key,
            client,
        })
    }
}

#[async_trait::async_trait]
impl Llm for OpenAi {
    async fn generate(&self, prompt: &str) -> crate::Result<String> {
        let messages = serde_json::json!([{ "role": "user", "content": prompt }]);
        let body = serde_json::json!({
            "model": self.model,
            "messages": messages,
            "temperature": 0.0,
            // A reasoning model spends this budget on its trace before it
            // writes any answer: the same prompt measured 347 completion
            // tokens with thinking on against 11 with it off. 512 left no room
            // for the verdict, so the run silently fell back to the no-LLM
            // path. There is no portable way to turn thinking off here -- an
            // unknown parameter is a hard 400 on strict servers -- so the fix
            // is headroom.
            "max_tokens": 2048,
        });

        let (mut status, mut text) = self.post(&body).await?;

        // OpenAI's own reasoning models (o-series, GPT-5) reject both of the
        // parameters above: `max_tokens` must be `max_completion_tokens`, and
        // `temperature` must be left at its default. Every other
        // OpenAI-compatible server in the wild -- vLLM, llama.cpp, LM Studio,
        // Ollama's shim -- speaks the older spelling and may not know the new
        // one, so we cannot simply switch. Retry once, driven by the server's
        // own complaint rather than by sniffing the model name (proxies rename
        // models freely), and only for this documented incompatibility.
        if status == reqwest::StatusCode::BAD_REQUEST && rejects_our_parameters(&text) {
            let retry = serde_json::json!({
                "model": self.model,
                "messages": messages,
                "max_completion_tokens": 2048,
            });
            (status, text) = self.post(&retry).await?;
        }

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
