//! Provider for any server with the OpenAI chat API: LM Studio, the llama.cpp
//! server, vLLM, `mlx_lm.server`, OpenRouter, Groq… and Ollama itself via
//! `/v1`.

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use bytes::Bytes;
use eventsource_stream::Eventsource;
use futures_util::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio_util::sync::CancellationToken;

use moon_core::{
    ChatEvent, ChatRequest, ChatStream, ConfigError, Health, Message, ModelInfo, Provider,
    ProviderConfig, ProviderError, ProviderFactory, Role, Usage,
};

const KIND: &str = "openai";

pub struct Factory;

impl ProviderFactory for Factory {
    fn kind(&self) -> &'static str {
        KIND
    }

    fn build(&self, id: &str, cfg: &ProviderConfig) -> Result<Arc<dyn Provider>, ConfigError> {
        let base_url = cfg.base_url.clone().ok_or_else(|| ConfigError::Provider {
            id: id.to_string(),
            detail: "missing `base_url` (e.g. http://localhost:1234/v1)".into(),
        })?;
        let api_key = match &cfg.api_key_env {
            Some(var) => match std::env::var(var) {
                Ok(v) if !v.trim().is_empty() => Some(v),
                _ => {
                    return Err(ConfigError::Provider {
                        id: id.to_string(),
                        detail: format!("environment variable {var} is not set"),
                    })
                }
            },
            None => None,
        };
        let p = OpenAiProvider::new(id, &base_url, api_key, cfg.timeout_secs).map_err(|e| {
            ConfigError::Provider {
                id: id.to_string(),
                detail: e.to_string(),
            }
        })?;
        Ok(Arc::new(p))
    }
}

pub struct OpenAiProvider {
    id: String,
    base_url: String,
    api_key: Option<String>,
    client: reqwest::Client,
    timeout: Duration,
}

impl OpenAiProvider {
    pub fn new(
        id: &str,
        base_url: &str,
        api_key: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Self, reqwest::Error> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .build()?;
        Ok(Self {
            id: id.to_string(),
            base_url: base_url.trim().trim_end_matches('/').to_string(),
            api_key,
            client,
            timeout: Duration::from_secs(timeout_secs.unwrap_or(15)),
        })
    }

    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        let mut rb = self
            .client
            .request(method, format!("{}{}", self.base_url, path));
        if let Some(k) = &self.api_key {
            rb = rb.bearer_auth(k);
        }
        rb
    }

    fn map_send_err(&self, e: reqwest::Error) -> ProviderError {
        if e.is_connect() || e.is_timeout() || e.is_request() {
            let mut cur: &dyn std::error::Error = &e;
            while let Some(next) = cur.source() {
                cur = next;
            }
            ProviderError::Unreachable {
                url: self.base_url.clone(),
                detail: cur.to_string(),
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
        let msg = error_message(&body);
        match status.as_u16() {
            401 | 403 => Err(ProviderError::Auth),
            404 if model.is_some() && msg.to_lowercase().contains("model") => {
                Err(ProviderError::ModelNotFound(model.unwrap().to_string()))
            }
            s => Err(ProviderError::Http {
                status: s,
                body: msg,
            }),
        }
    }
}

fn error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(m) = v.pointer("/error/message").and_then(|m| m.as_str()) {
            return m.to_string();
        }
        if let Some(m) = v.get("error").and_then(|m| m.as_str()) {
            return m.to_string();
        }
    }
    body.chars().take(200).collect()
}

#[derive(Deserialize)]
struct ModelList {
    #[serde(default)]
    data: Vec<ModelEntry>,
}

#[derive(Deserialize)]
struct ModelEntry {
    id: String,
}

#[derive(Serialize)]
struct WireMessage {
    role: &'static str,
    content: String,
}

#[derive(Deserialize, Default)]
struct Chunk {
    #[serde(default)]
    choices: Vec<Choice>,
    usage: Option<WireUsage>,
}

#[derive(Deserialize)]
struct Choice {
    delta: Option<Delta>,
}

#[derive(Deserialize)]
struct Delta {
    content: Option<String>,
    reasoning_content: Option<String>,
    reasoning: Option<String>,
}

#[derive(Deserialize)]
struct WireUsage {
    prompt_tokens: Option<u32>,
    completion_tokens: Option<u32>,
}

fn build_body(req: &ChatRequest) -> Value {
    let p = &req.params;
    let mut body = Map::new();
    body.insert("model".into(), req.model.clone().into());
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m: &Message| {
            serde_json::to_value(WireMessage {
                role: match m.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                },
                content: m.wire_content(),
            })
            .unwrap_or(Value::Null)
        })
        .collect();
    body.insert("messages".into(), messages.into());
    body.insert("stream".into(), true.into());
    body.insert(
        "stream_options".into(),
        serde_json::json!({"include_usage": true}),
    );
    if let Some(v) = p.temperature {
        body.insert("temperature".into(), v.into());
    }
    if let Some(v) = p.top_p {
        body.insert("top_p".into(), v.into());
    }
    if let Some(v) = p.max_tokens {
        body.insert("max_tokens".into(), v.into());
    }
    if !p.stop.is_empty() {
        body.insert("stop".into(), p.stop.clone().into());
    }
    for (k, v) in &p.extra {
        if let Ok(j) = serde_json::to_value(v) {
            body.insert(k.clone(), j);
        }
    }
    Value::Object(body)
}

#[async_trait]
impl Provider for OpenAiProvider {
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
        let n = self.list_models().await?.len();
        Ok(Health {
            version: None,
            detail: Some(format!("{n} models")),
        })
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        let resp = self
            .request(reqwest::Method::GET, "/models")
            .timeout(self.timeout)
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
        let resp = self.check(resp, None).await?;
        let list: ModelList = resp
            .json()
            .await
            .map_err(|e| ProviderError::Decode(e.to_string()))?;
        let mut models: Vec<ModelInfo> = list
            .data
            .into_iter()
            .map(|m| ModelInfo::new(&self.id, m.id))
            .collect();
        models.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(models)
    }

    async fn chat(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ChatStream, ProviderError> {
        tracing::debug!(model = %req.model, messages = req.messages.len(), "openai /chat/completions");
        let resp = self
            .request(reqwest::Method::POST, "/chat/completions")
            .json(&build_body(&req))
            .send()
            .await
            .map_err(|e| self.map_send_err(e))?;
        let resp = self.check(resp, Some(&req.model)).await?;
        Ok(chat_stream(resp.bytes_stream(), cancel))
    }
}

/// Turns the SSE of `/chat/completions` into events. Kept separate from the
/// HTTP layer so it can be tested with arbitrary chunks.
pub fn chat_stream<S, E>(body: S, cancel: CancellationToken) -> ChatStream
where
    S: Stream<Item = Result<Bytes, E>> + Send + 'static,
    E: std::error::Error + Send + Sync + 'static,
{
    Box::pin(async_stream::try_stream! {
        let body = Box::pin(body);
        let mut events = body.eventsource();
        let started = Instant::now();
        let mut usage = Usage::default();
        let mut tokens: u32 = 0;
        loop {
            let next = tokio::select! {
                ev = events.next() => Ok(ev),
                _ = cancel.cancelled() => Err(ProviderError::Cancelled),
            };
            let Some(ev) = next? else { break };
            let ev = ev.map_err(|e| ProviderError::Decode(e.to_string()))?;
            let data = ev.data.trim();
            if data.is_empty() {
                continue;
            }
            if data == "[DONE]" {
                break;
            }
            let chunk: Chunk = serde_json::from_str(data)
                .map_err(|e| ProviderError::Decode(format!("{e}: {data}")))?;
            if let Some(u) = chunk.usage {
                usage.prompt_tokens = u.prompt_tokens;
                usage.completion_tokens = u.completion_tokens;
            }
            for choice in chunk.choices {
                let Some(delta) = choice.delta else { continue };
                if let Some(t) = delta.reasoning_content.or(delta.reasoning).filter(|t| !t.is_empty()) {
                    yield ChatEvent::Thinking(t);
                }
                if let Some(c) = delta.content.filter(|c| !c.is_empty()) {
                    tokens += 1;
                    yield ChatEvent::Delta(c);
                }
            }
        }
        let secs = started.elapsed().as_secs_f32();
        let n = usage.completion_tokens.unwrap_or(tokens);
        if secs > 0.0 && n > 0 {
            usage.tokens_per_second = Some(n as f32 / secs);
        }
        usage.total_duration_ms = Some(started.elapsed().as_millis() as u64);
        yield ChatEvent::Done(usage);
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::stream;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn chunks(parts: &[&str]) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
        let v: Vec<Result<Bytes, std::io::Error>> = parts
            .iter()
            .map(|p| Ok(Bytes::from(p.to_string())))
            .collect();
        stream::iter(v)
    }

    #[tokio::test]
    async fn sse_troceado() {
        let s = chunks(&[
            "data: {\"choices\":[{\"delta\":{\"content\":\"Ho\"}}]}\n\ndata: {\"choices\":[{\"delta\":{\"cont",
            "ent\":\"la\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"reasoning_content\":\"pienso\"}}]}\n\n",
            "data: {\"choices\":[],\"usage\":{\"prompt_tokens\":5,\"completion_tokens\":2}}\n\n",
            "data: [DONE]\n\n",
        ]);
        let events: Vec<_> = chat_stream(s, CancellationToken::new())
            .map(|e| e.unwrap())
            .collect()
            .await;
        assert_eq!(events[0], ChatEvent::Delta("Ho".into()));
        assert_eq!(events[1], ChatEvent::Delta("la".into()));
        assert_eq!(events[2], ChatEvent::Thinking("pienso".into()));
        match &events[3] {
            ChatEvent::Done(u) => {
                assert_eq!(u.prompt_tokens, Some(5));
                assert_eq!(u.completion_tokens, Some(2));
            }
            other => panic!("esperaba Done, llegó {other:?}"),
        }
    }

    #[tokio::test]
    async fn modelos_auth_y_bearer() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .and(header("authorization", "Bearer k"))
            .respond_with(
                ResponseTemplate::new(200)
                    .set_body_json(serde_json::json!({"data": [{"id": "b"}, {"id": "a"}]})),
            )
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/v1/models"))
            .respond_with(
                ResponseTemplate::new(401)
                    .set_body_json(serde_json::json!({"error": {"message": "bad key"}})),
            )
            .mount(&server)
            .await;
        let base = format!("{}/v1", server.uri());
        let ok = OpenAiProvider::new("lm", &base, Some("k".into()), None).unwrap();
        let models = ok.list_models().await.unwrap();
        assert_eq!(
            models.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
        let bad = OpenAiProvider::new("lm", &base, None, None).unwrap();
        assert!(matches!(bad.health().await, Err(ProviderError::Auth)));
    }

    #[tokio::test]
    async fn chat_por_http() {
        let server = MockServer::start().await;
        let body = "data: {\"choices\":[{\"delta\":{\"content\":\"hey\"}}]}\n\ndata: [DONE]\n\n";
        Mock::given(method("POST"))
            .and(path("/v1/chat/completions"))
            .respond_with(ResponseTemplate::new(200).set_body_raw(body, "text/event-stream"))
            .mount(&server)
            .await;
        let p = OpenAiProvider::new("lm", &format!("{}/v1", server.uri()), None, None).unwrap();
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![Message::user("hola")],
            params: Default::default(),
        };
        let events: Vec<_> = p
            .chat(req, CancellationToken::new())
            .await
            .unwrap()
            .map(|e| e.unwrap())
            .collect()
            .await;
        assert_eq!(events[0], ChatEvent::Delta("hey".into()));
        assert!(matches!(events[1], ChatEvent::Done(_)));
    }

    #[test]
    fn cuerpo() {
        let req = ChatRequest {
            model: "m".into(),
            messages: vec![Message::system("s"), Message::user("u")],
            params: moon_core::GenerationParams {
                temperature: Some(0.1),
                max_tokens: Some(9),
                ..Default::default()
            },
        };
        let b = build_body(&req);
        assert_eq!(b["model"], "m");
        assert_eq!(b["messages"][0]["role"], "system");
        assert!((b["temperature"].as_f64().unwrap() - 0.1).abs() < 1e-6);
        assert_eq!(b["max_tokens"], 9);
        assert_eq!(b["stream"], true);
    }
}
