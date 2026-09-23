//! JSON shapes of the Ollama API. Only the fields that are used.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use moon_core::{Message, Role, ToolCall, ToolSpec};

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
    /// `{"type": "function", "function": {name, description, parameters}}`
    /// each; left out when there are none, so the request is what it always was.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Value>,
}

#[derive(Serialize)]
pub struct WireMessage {
    pub role: &'static str,
    pub content: String,
    /// The calls an assistant message made, echoed back in the history.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<Value>>,
    /// On a tool message: which tool answered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// On a tool message: the call it answers, when Ollama gave it an id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

pub fn tool_spec(t: &ToolSpec) -> Value {
    serde_json::json!({
        "type": "function",
        "function": {"name": t.name, "description": t.description, "parameters": t.parameters}
    })
}

fn tool_call(c: &ToolCall) -> Value {
    let mut v = serde_json::json!({"function": {"name": c.name, "arguments": c.arguments}});
    if let Some(id) = &c.id {
        v["id"] = Value::String(id.clone());
    }
    v
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
            tools: req.tools.iter().map(tool_spec).collect(),
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
            tool_calls: (!m.tool_calls.is_empty())
                .then(|| m.tool_calls.iter().map(tool_call).collect()),
            tool_name: m.tool_name.clone(),
            tool_call_id: m.tool_call_id.clone(),
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
    #[serde(default)]
    pub tool_calls: Vec<WireToolCall>,
}

#[derive(Deserialize)]
pub struct WireToolCall {
    /// Newer Ollama versions give every call an id; older ones do not.
    pub id: Option<String>,
    pub function: WireFunction,
}

#[derive(Deserialize)]
pub struct WireFunction {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

impl From<WireToolCall> for ToolCall {
    fn from(c: WireToolCall) -> Self {
        ToolCall {
            id: c.id.filter(|i| !i.is_empty()),
            name: c.function.name,
            arguments: arguments_value(c.function.arguments),
        }
    }
}

/// The arguments are an object, but some models hand them over as a JSON
/// string: that is parsed; anything else is passed on for the tool to refuse.
pub fn arguments_value(v: Value) -> Value {
    match v {
        Value::String(s) => serde_json::from_str(&s).unwrap_or(Value::String(s)),
        Value::Null => Value::Object(Map::new()),
        other => other,
    }
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
