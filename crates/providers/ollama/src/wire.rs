//! JSON shapes of the Ollama API. Only the fields that are used.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use moon_core::{Message, Role};

#[derive(Deserialize)]
pub struct Version {
    pub version: String,
}

#[derive(Deserialize)]
pub struct Tags {
    #[serde(default)]
    pub models: Vec<TagModel>,
}

#[derive(Deserialize)]
pub struct TagModel {
    pub name: String,
    pub size: Option<u64>,
    pub details: Option<Details>,
}

#[derive(Deserialize)]
pub struct Details {
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization_level: Option<String>,
}

/// `/api/ps`: the models in memory. `size` is what it takes up loaded (weights
/// plus context cache) and `size_vram` how much of that is on the GPU.
#[derive(Deserialize)]
pub struct Ps {
    #[serde(default)]
    pub models: Vec<PsModel>,
}

#[derive(Deserialize)]
pub struct PsModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub size_vram: u64,
    pub context_length: Option<u32>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Serialize)]
pub struct ShowRequest<'a> {
    pub model: &'a str,
}

#[derive(Deserialize)]
pub struct Show {
    #[serde(default)]
    pub model_info: Map<String, Value>,
    pub details: Option<Details>,
    pub capabilities: Option<Vec<String>>,
}

#[derive(Serialize)]
pub struct ChatRequest<'a> {
    pub model: &'a str,
    pub messages: Vec<WireMessage>,
    pub stream: bool,
    #[serde(skip_serializing_if = "Map::is_empty")]
    pub options: Map<String, Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub think: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<&'a str>,
}

#[derive(Serialize)]
pub struct WireMessage {
    pub role: &'static str,
    pub content: String,
}

impl<'a> ChatRequest<'a> {
    pub fn from_core(
        req: &'a moon_core::ChatRequest,
        think_default: Option<bool>,
        keep_alive: Option<&'a str>,
    ) -> Self {
        let p = &req.params;
        let mut options = Map::new();
        if let Some(v) = p.temperature {
            options.insert("temperature".into(), v.into());
        }
        if let Some(v) = p.top_p {
            options.insert("top_p".into(), v.into());
        }
        if let Some(v) = p.max_tokens {
            options.insert("num_predict".into(), v.into());
        }
        if let Some(v) = p.num_ctx {
            options.insert("num_ctx".into(), v.into());
        }
        if !p.stop.is_empty() {
            options.insert("stop".into(), p.stop.clone().into());
        }
        for (k, v) in &p.extra {
            options.insert(k.clone(), serde_json::to_value(v).unwrap_or(Value::Null));
        }
        Self {
            model: &req.model,
            messages: req.messages.iter().map(WireMessage::from).collect(),
            stream: true,
            options,
            think: p.think.or(think_default),
            keep_alive,
        }
    }
}

impl From<&Message> for WireMessage {
    fn from(m: &Message) -> Self {
        Self {
            role: match m.role {
                Role::System => "system",
                Role::User => "user",
                Role::Assistant => "assistant",
                Role::Tool => "tool",
            },
            content: m.wire_content(),
        }
    }
}

#[derive(Deserialize)]
pub struct ChatChunk {
    pub message: Option<ChunkMessage>,
    #[serde(default)]
    pub done: bool,
    pub error: Option<String>,
    pub total_duration: Option<u64>,
    pub load_duration: Option<u64>,
    pub prompt_eval_count: Option<u32>,
    pub eval_count: Option<u32>,
    pub eval_duration: Option<u64>,
}

#[derive(Deserialize)]
pub struct ChunkMessage {
    #[serde(default)]
    pub content: String,
    pub thinking: Option<String>,
}

/// Ollama returns `{"error": "..."}`; otherwise the body as is, truncated.
pub fn error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        if let Some(e) = v.get("error").and_then(|e| e.as_str()) {
            return e.to_string();
        }
    }
    body.chars().take(200).collect()
}
