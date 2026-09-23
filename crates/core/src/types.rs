use std::collections::BTreeMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::context::Attachment;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Role::System => "system",
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        }
    }
}

/// One message of the conversation. It is what gets persisted in the session
/// and what gets sent to the provider (only `role` and `content`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Message {
    pub role: Role,
    pub content: String,
    #[serde(default = "Utc::now")]
    pub ts: DateTime<Utc>,
    /// Model that generated the reply (only on assistant messages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,
    /// The model's reasoning, if it exposes it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking: Option<String>,
    /// The generation was cancelled before finishing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub partial: bool,
    /// Files attached with `@path` (snapshots; only on user messages).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Tools the model asked for (only on assistant messages).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Which tool this message answers (only on tool messages).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    /// The call it answers, when the provider gave the call an id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            ts: Utc::now(),
            model: None,
            usage: None,
            thinking: None,
            partial: false,
            attachments: Vec::new(),
            tool_calls: Vec::new(),
            tool_name: None,
            tool_call_id: None,
        }
    }

    /// Content the provider sees: the attachment blocks and the text.
    pub fn wire_content(&self) -> String {
        if self.attachments.is_empty() {
            return self.content.clone();
        }
        let mut out: Vec<String> = self.attachments.iter().map(Attachment::block).collect();
        out.push(self.content.clone());
        out.join("\n\n")
    }

    pub fn system(content: impl Into<String>) -> Self {
        Self::new(Role::System, content)
    }

    pub fn user(content: impl Into<String>) -> Self {
        Self::new(Role::User, content)
    }

    pub fn assistant(content: impl Into<String>) -> Self {
        Self::new(Role::Assistant, content)
    }

    /// What a tool returned, for the model to read on the next request.
    pub fn tool(
        name: impl Into<String>,
        call_id: Option<String>,
        content: impl Into<String>,
    ) -> Self {
        let mut m = Self::new(Role::Tool, content);
        m.tool_name = Some(name.into());
        m.tool_call_id = call_id;
        m
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Usage {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub completion_tokens: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub load_duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens_per_second: Option<f32>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Capabilities {
    pub streaming: bool,
    pub tools: bool,
    pub vision: bool,
    pub thinking: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModelInfo {
    /// Id as the provider knows it: "qwen2.5-coder:14b".
    pub id: String,
    /// Id of the provider instance: "ollama".
    pub provider: String,
    pub context_length: Option<u32>,
    pub size_bytes: Option<u64>,
    pub family: Option<String>,
    pub parameter_size: Option<String>,
    pub quantization: Option<String>,
    pub caps: Capabilities,
}

impl ModelInfo {
    pub fn new(provider: impl Into<String>, id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            provider: provider.into(),
            context_length: None,
            size_bytes: None,
            family: None,
            parameter_size: None,
            quantization: None,
            caps: Capabilities {
                streaming: true,
                ..Capabilities::default()
            },
        }
    }

    /// Qualified id, unique across providers: "ollama/qwen2.5-coder:14b".
    pub fn qualified(&self) -> String {
        format!("{}/{}", self.provider, self.id)
    }
}

/// Generation parameters. All optional: whatever is not set is up to the
/// provider. `extra` collects provider-specific keys.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GenerationParams {
    pub temperature: Option<f32>,
    pub top_p: Option<f32>,
    pub max_tokens: Option<u32>,
    pub num_ctx: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub stop: Vec<String>,
    pub think: Option<bool>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, toml::Value>,
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<Message>,
    pub params: GenerationParams,
    /// Tools the model may call. Empty: none are offered, as before tools existed.
    pub tools: Vec<ToolSpec>,
}

/// A tool as the model sees it: a name, what it is for and the JSON schema
/// of its arguments. The providers wrap it in their own envelope.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// A call the model made. Ollama gives calls no id; the OpenAI API does and
/// wants it back on the tool message.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolCall {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatEvent {
    Delta(String),
    Thinking(String),
    ToolCall(ToolCall),
    Done(Usage),
}

/// A model the provider has loaded in memory right now. What it takes up is
/// not its size on disk: loading it also reserves the context cache, so it
/// depends on `num_ctx` as much as on the model.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedModel {
    /// Model id, as the provider knows it.
    pub id: String,
    /// What it takes up loaded: weights plus context cache.
    pub size_bytes: u64,
    /// Of that, what sits in GPU memory.
    pub size_vram_bytes: u64,
    /// Context window it was loaded with.
    pub context_length: Option<u32>,
    /// When the provider will unload it if unused (`keep_alive`).
    pub expires_at: Option<DateTime<Utc>>,
}

impl LoadedModel {
    /// Share of the model that does not fit in the GPU, as a percentage. Once
    /// it goes above zero, generation crawls.
    pub fn cpu_percent(&self) -> u32 {
        if self.size_bytes == 0 {
            return 0;
        }
        let cpu = self.size_bytes.saturating_sub(self.size_vram_bytes);
        (cpu as f64 / self.size_bytes as f64 * 100.0).round() as u32
    }

    /// Share of the model that sits in the GPU, as a percentage.
    pub fn gpu_percent(&self) -> u32 {
        100 - self.cpu_percent()
    }

    /// Time left until the provider unloads it, if known.
    pub fn expires_in(&self) -> Option<std::time::Duration> {
        self.expires_at.and_then(|t| (t - Utc::now()).to_std().ok())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Health {
    pub version: Option<String>,
    pub detail: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(size: u64, vram: u64) -> LoadedModel {
        LoadedModel {
            id: "m".into(),
            size_bytes: size,
            size_vram_bytes: vram,
            context_length: None,
            expires_at: None,
        }
    }

    #[test]
    fn split_between_gpu_and_cpu() {
        // fully in the GPU: nothing on the CPU, and nothing gets drawn
        let m = loaded(1000, 1000);
        assert_eq!(m.cpu_percent(), 0);
        assert_eq!(m.gpu_percent(), 100);
        // part outside: that is what drags generation down
        let m = loaded(1000, 700);
        assert_eq!(m.cpu_percent(), 30);
        assert_eq!(m.gpu_percent(), 70);
        // a machine without a GPU has it all on the CPU
        let m = loaded(1000, 0);
        assert_eq!(m.cpu_percent(), 100);
        // an unknown size does not divide by zero
        assert_eq!(loaded(0, 0).cpu_percent(), 0);
    }

    #[test]
    fn tool_fields_round_trip_and_old_records_still_load() {
        let mut m = Message::assistant("");
        m.tool_calls.push(ToolCall {
            id: Some("c1".into()),
            name: "read_file".into(),
            arguments: serde_json::json!({"path": "a.rs"}),
        });
        let json = serde_json::to_string(&m).unwrap();
        assert!(json.contains("\"tool_calls\""));
        let back: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(back, m);
        let t = Message::tool("read_file", Some("c1".into()), "ok");
        assert_eq!(t.role, Role::Tool);
        assert_eq!(t.tool_name.as_deref(), Some("read_file"));
        // a message written before tools existed has none of the fields
        let old: Message =
            serde_json::from_str(r#"{"role":"user","content":"hi","ts":"2026-01-01T00:00:00Z"}"#)
                .unwrap();
        assert!(old.tool_calls.is_empty());
        assert!(old.tool_name.is_none());
        assert!(!serde_json::to_string(&old).unwrap().contains("tool"));
    }

    #[test]
    fn a_past_expiry_does_not_count() {
        let mut m = loaded(10, 10);
        m.expires_at = Some(Utc::now() - chrono::Duration::seconds(30));
        assert!(m.expires_in().is_none());
        m.expires_at = Some(Utc::now() + chrono::Duration::seconds(100));
        assert!(m.expires_in().is_some_and(|d| d.as_secs() >= 95));
    }
}
