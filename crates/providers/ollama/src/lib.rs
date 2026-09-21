//! Ollama provider over its native API (`/api/*`), which gives more than the
//! OpenAI-compatible layer: size, quantization, context and durations.

mod wire;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use bytes::Bytes;
use futures_util::{Stream, TryStreamExt};
use tokio::io::AsyncBufReadExt;
use tokio_util::io::StreamReader;
use tokio_util::sync::CancellationToken;

use moon_core::{
    Capabilities, ChatEvent, ChatRequest, ChatStream, ConfigError, Health, LoadedModel, ModelInfo,
    Provider, ProviderConfig, ProviderError, ProviderFactory, Usage,
};

pub const DEFAULT_URL: &str = "http://localhost:11434";
const KIND: &str = "ollama";

pub struct Factory;

impl ProviderFactory for Factory {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, id: &str, cfg: &ProviderConfig) -> Result<Arc<dyn Provider>, ConfigError> {
        let raw = cfg
            .base_url
            .clone()
            .or_else(|| {
                std::env::var("OLLAMA_HOST")
                    .ok()
                    .filter(|s| !s.trim().is_empty())
            })
            .unwrap_or_else(|| DEFAULT_URL.to_string());
        let think = cfg.extra.get("think").and_then(|v| v.as_bool());
        let keep_alive = cfg
            .extra
            .get("keep_alive")
            .and_then(|v| v.as_str())
            .map(String::from);
        let p = OllamaProvider::new(
            id,
            &normalize_url(&raw),
            cfg.timeout_secs,
            think,
            keep_alive,
        )
        .map_err(|e| ConfigError::Provider {
            id: id.to_string(),
            detail: e.to_string(),
        })?;
        Ok(Arc::new(p))
    }
}

/// Accepts what `OLLAMA_HOST` accepts: "host:port", ":11434", with or without
/// a scheme.
pub fn normalize_url(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches('/').to_string();
    if s.starts_with(':') {
        s = format!("localhost{s}");
    }
    if !s.contains("://") {
        s = format!("http://{s}");
    }
    s.replace("://0.0.0.0", "://localhost")
}

pub struct OllamaProvider {
    id: String,
    base_url: String,
    client: reqwest::Client,
    think: Option<bool>,
    keep_alive: Option<String>,
    timeout: Duration,
}

impl OllamaProvider {
    pub fn new(
        id: &str,
        base_url: &str,
        timeout_secs: Option<u64>,
        think: Option<bool>,
        keep_alive: Option<String>,
    ) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()?;
        Ok(Self {
            id: id.to_string(),
            base_url: base_url.to_string(),
            client,
            think,
            keep_alive,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(15)),
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}{}", self.base_url, path)
    }

    fn map_send_err(&self, e: reqwest::Error) -> ProviderError {
        if e.is_connect() || e.is_timeout() || e.is_request() {
            ProviderError::Unreachable {
                url: self.base_url.clone(),
                detail: root_cause(&e),
            }
        } else {
            ProviderError::Decode(e.to_string())
        }
    }

    async fn check(
        &self,
        resp: reqwest::Response,
        model: Option<&str>,
    ) -> Result<reqwest::Response, ProviderError> {
        let status = resp.status();
        if status.is_success() {
            return Ok(resp);
        }
        let body = resp.text().await.unwrap_or_default();
        if status.as_u16() == 404 {
            if let Some(m) = model {
                if body.contains("not found") {
                    return Err(ProviderError::ModelNotFound(m.to_string()));
                }
            }
        }
        Err(ProviderError::Http {
            status: status.as_u16(),
            body: wire::error_message(&body),
        })
    }
}

fn root_cause(e: &reqwest::Error) -> String {
    let mut cur: &dyn std::error::Error = e;
    while let Some(next) = cur.source() {
        cur = next;
    }
    cur.to_string()
}

#[async_trait]
impl Provider for OllamaProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn kind(&self) -> &'static str {
        KIND
    }

    fn base_url(&self) -> &str {
        &self.base_url
    }

    async fn health(&self) -> Result<Health, ProviderError> {
        let resp = self
            .client
            .get(self.url("/api/version"))
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
        let resp = self.check(resp, None).await?;
        let v: wire::Version = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        Ok(Health {
            version: Some(v.version),
            detail: None,
        })
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = self
            .client
            .get(self.url("/api/tags"))
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
        let resp = self.check(resp, None).await?;
        let tags: wire::Tags = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        let mut models: Vec<ModelInfo> = tags
            .models
            .into_iter()
            .map(|m| {
                let mut info = ModelInfo::new(&self.id, m.name);
                info.size_bytes = m.size;
                if let Some(d) = m.details {
                    info.family = d.family;
                    info.parameter_size = d.parameter_size;
                    info.quantization = d.quantization_level;
                }
                info
            })
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }

    async fn loaded(&self) -> Result<Option<Vec<LoadedModel>>, ProviderError> {
        let resp = self
            .client
            .get(self.url("/api/ps"))
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
        let resp = self.check(resp, None).await?;
        let ps: wire::Ps = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        Ok(Some(
            ps.models
                .into_iter()
                .map(|m| LoadedModel {
                    id: m.name,
                    size_bytes: m.size,
                    size_vram_bytes: m.size_vram,
                    context_length: m.context_length,
                    expires_at: m.expires_at,
                })
                .collect(),
        ))
    }

    async fn model_info(&self, model: &str) -> Result<ModelInfo, ProviderError> {
        let resp = self
            .client
            .post(self.url("/api/show"))
            .timeout(self.timeout)
            .json(&wire::ShowRequest { model })
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
        let resp = self.check(resp, Some(model)).await?;
        let show: wire::Show = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        let mut info = ModelInfo::new(&self.id, model);
        info.context_length = show
            .model_info
            .iter()
            .find(|(k, _)| k.ends_with(".context_length"))
            .and_then(|(_, v)| v.as_u64())
            .map(|v| v as u32);
        if let Some(d) = show.details {
            info.family = d.family;
            info.parameter_size = d.parameter_size;
            info.quantization = d.quantization_level;
        }
        let caps: Vec<String> = show.capabilities.unwrap_or_default();
        info.caps = Capabilities {
            streaming: true,
            tools: caps.iter().any(|c| c == "tools"),
            vision: caps.iter().any(|c| c == "vision"),
            thinking: caps.iter().any(|c| c == "thinking"),
        };
        Ok(info)
    }

    async fn chat(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ChatStream, ProviderError> {
        let body = wire::ChatRequest::from_core(&req, self.think, self.keep_alive.as_deref());
        tracing::debug!(model = %req.model, messages = req.messages.len(), "ollama /api/chat");
        let resp = self
            .client
            .post(self.url("/api/chat"))
            .json(&body)
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
        let resp = self.check(resp, Some(&req.model)).await?;
        Ok(chat_stream(resp.bytes_stream(), cancel))
    }
}

/// Turns the NDJSON body of `/api/chat` into events. Kept separate from the
/// HTTP layer so it can be tested with chunks cut mid-line.
pub fn chat_stream<S, E>(body: S, cancel: CancellationToken) -> ChatStream
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    Box::pin(async_stream::try_stream! {
        let body = Box::pin(body);
        let reader = StreamReader::new(body.map_err(std::io::Error::other));
        let mut lines = reader.lines();
        loop {
            let next = tokio::select! {
                l = lines.next_line() => l.map_err(|e| ProviderError::Decode(e.to_string())),
                _ = cancel.cancelled() => Err(ProviderError::Cancelled),
            };
            let Some(line) = next? else { break };
            if line.trim().is_empty() {
                continue;
            }
            let mut chunk: wire::ChatChunk = serde_json::from_str(&line)
                .map_err(|e| ProviderError::Decode(format!("{e}: {line}")))?;
            if let Some(err) = chunk.error.take() {
                Err(ProviderError::Http { status: 200, body: err })?;
            }
            if let Some(msg) = chunk.message.take() {
                if let Some(t) = msg.thinking.filter(|t| !t.is_empty()) {
                    yield ChatEvent::Thinking(t);
                }
                if !msg.content.is_empty() {
                    yield ChatEvent::Delta(msg.content);
                }
            }
            if chunk.done {
                yield ChatEvent::Done(usage_from(&chunk));
                break;
            }
        }
    })
}

fn usage_from(c: &wire::ChatChunk) -> Usage {
    let tps = match (c.eval_count, c.eval_duration) {
        (Some(n), Some(d)) if d > 0 => Some(n as f32 / (d as f32 / 1e9)),
        _ => None,
    };
    Usage {
        prompt_tokens: c.prompt_eval_count,
        completion_tokens: c.eval_count,
        total_duration_ms: c.total_duration.map(|d| d / 1_000_000),
        load_duration_ms: c.load_duration.map(|d| d / 1_000_000),
        tokens_per_second: tps,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{stream, StreamExt};
    use moon_core::Message;
    use wiremock::matchers::{body_partial_json, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn chunks(parts: &[&str]) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
        let v: Vec<Result<Bytes, std::io::Error>> = parts
            .iter()
            .map(|p| Ok(Bytes::from(p.to_string())))
            .collect();
        stream::iter(v)
    }

    #[tokio::test]
    async fn ndjson_cut_mid_line() {
        let s = chunks(&[
            "{\"message\":{\"role\":\"assistant\",\"content\":\"He\"},\"done\":false}\n{\"message\":{\"role\":\"assist",
            "ant\",\"content\":\"llo\"},\"done\":false}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\",\"thinking\":\"hm\"},\"done\":false}\n",
            "{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true,\"eval_count\":10,\"eval_duration\":500000000,\"prompt_eval_count\":3}\n",
        ]);
        let events: Vec<_> = chat_stream(s, CancellationToken::new())
            .map(|e| e.unwrap())
            .collect()
            .await;
        assert_eq!(events[0], ChatEvent::Delta("He".into()));
        assert_eq!(events[1], ChatEvent::Delta("llo".into()));
        assert_eq!(events[2], ChatEvent::Thinking("hm".into()));
        match &events[3] {
            ChatEvent::Done(u) => {
                assert_eq!(u.completion_tokens, Some(10));
                assert_eq!(u.prompt_tokens, Some(3));
                assert_eq!(u.tokens_per_second, Some(20.0));
            }
            other => panic!("expected Done, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn cancellation() {
        let cancel = CancellationToken::new();
        cancel.cancel();
        let s = stream::pending::<Result<Bytes, std::io::Error>>();
        let mut st = chat_stream(s, cancel);
        match st.next().await {
            Some(Err(ProviderError::Cancelled)) => {}
            other => panic!("expected Cancelled, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn error_on_a_line() {
        let s = chunks(&["{\"error\":\"model requires more memory\"}\n"]);
        let mut st = chat_stream(s, CancellationToken::new());
        match st.next().await {
            Some(Err(ProviderError::Http { body, .. })) => assert!(body.contains("memory")),
            other => panic!("expected Http, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn ps_reports_what_the_loaded_model_takes() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [{"name": "qwen3:4b", "model": "qwen3:4b", "size": 5329597235u64,
                            "size_vram": 5329597235u64, "context_length": 32768,
                            "expires_at": "2126-09-19T00:18:05.927705+02:00"}]
            })))
            .mount(&server)
            .await;
        let p = OllamaProvider::new("ollama", &server.uri(), None, None, None).unwrap();
        let list = p
            .loaded()
            .await
            .unwrap()
            .expect("ollama can report loaded models");
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "qwen3:4b");
        // the loaded size is more than on disk: it includes the context cache
        assert_eq!(list[0].size_bytes, 5329597235);
        assert_eq!(list[0].context_length, Some(32768));
        assert_eq!(list[0].cpu_percent(), 0);
        assert!(list[0].expires_in().is_some());
    }

    #[tokio::test]
    async fn an_empty_ps_means_the_model_is_unloaded() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/api/ps"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"models": []})),
            )
            .mount(&server)
            .await;
        let p = OllamaProvider::new("ollama", &server.uri(), None, None, None).unwrap();
        assert_eq!(p.loaded().await.unwrap(), Some(Vec::new()));
    }

    #[tokio::test]
    async fn tags_and_show() {
        let server = MockServer::start().await;
        Mock::given(method("GET")).and(path("/api/tags"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "models": [{"name": "qwen2.5-coder:14b", "model": "qwen2.5-coder:14b", "size": 8988124069u64,
                            "details": {"family": "qwen2", "parameter_size": "14.8B", "quantization_level": "Q4_K_M"}}]
            })))
            .mount(&server).await;
        Mock::given(method("POST"))
            .and(path("/api/show"))
            .and(body_partial_json(
                serde_json::json!({"model": "qwen2.5-coder:14b"}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "model_info": {"qwen2.context_length": 32768, "general.architecture": "qwen2"},
                "capabilities": ["completion", "tools"]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/api/version"))
            .respond_with(
                ResponseTemplate::new(200).set_body_json(serde_json::json!({"version": "0.34.0"})),
            )
            .mount(&server)
            .await;

        let p = OllamaProvider::new("ollama", &server.uri(), None, None, None).unwrap();
        assert_eq!(p.health().await.unwrap().version.as_deref(), Some("0.34.0"));
        let models = p.list_models().await.unwrap();
        assert_eq!(models.len(), 1);
        assert_eq!(models[0].qualified(), "ollama/qwen2.5-coder:14b");
        assert_eq!(models[0].quantization.as_deref(), Some("Q4_K_M"));
        let info = p.model_info("qwen2.5-coder:14b").await.unwrap();
        assert_eq!(info.context_length, Some(32768));
        assert!(info.caps.tools);
        assert!(!info.caps.thinking);
    }

    #[tokio::test]
    async fn full_chat_and_a_missing_model() {
        let server = MockServer::start().await;
        let body = "{\"message\":{\"role\":\"assistant\",\"content\":\"hello\"},\"done\":false}\n{\"message\":{\"role\":\"assistant\",\"content\":\"\"},\"done\":true}\n";
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(
                serde_json::json!({"model": "x", "stream": true}),
            ))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "application/x-ndjson"))
            .mount(&server)
            .await;
        Mock::given(method("POST"))
            .and(path("/api/chat"))
            .and(body_partial_json(serde_json::json!({"model": "nope"})))
            .respond_with(
                ResponseTemplate::new(404)
                    .set_body_json(serde_json::json!({"error": "model 'nope' not found"})),
            )
            .mount(&server)
            .await;

        let p = OllamaProvider::new("ollama", &server.uri(), None, None, None).unwrap();
        let req = ChatRequest {
            model: "x".into(),
            messages: vec![Message::user("hello")],
            params: Default::default(),
        };
        let events: Vec<_> = p
            .chat(req, CancellationToken::new())
            .await
            .unwrap()
            .map(|e| e.unwrap())
            .collect()
            .await;
        assert_eq!(events[0], ChatEvent::Delta("hello".into()));
        assert!(matches!(events[1], ChatEvent::Done(_)));

        let req = ChatRequest {
            model: "nope".into(),
            messages: vec![],
            params: Default::default(),
        };
        match p.chat(req, CancellationToken::new()).await {
            Err(ProviderError::ModelNotFound(m)) => assert_eq!(m, "nope"),
            other => panic!("expected ModelNotFound, got {:?}", other.map(|_| ())),
        }
    }

    #[tokio::test]
    async fn with_no_server_it_is_unreachable() {
        let p = OllamaProvider::new("ollama", "http://127.0.0.1:1", None, None, None).unwrap();
        match p.health().await {
            Err(ProviderError::Unreachable { url, .. }) => assert_eq!(url, "http://127.0.0.1:1"),
            other => panic!("expected Unreachable, got {other:?}"),
        }
    }

    #[test]
    fn urls() {
        assert_eq!(normalize_url("localhost:11434/"), "http://localhost:11434");
        assert_eq!(normalize_url(":11434"), "http://localhost:11434");
        assert_eq!(normalize_url("https://x.y"), "https://x.y");
        assert_eq!(normalize_url("0.0.0.0:11434"), "http://localhost:11434");
    }
}
