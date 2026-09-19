use std::sync::Arc;

use async_trait::async_trait;
use futures_util::stream::BoxStream;
use tokio_util::sync::CancellationToken;

use crate::config::ProviderConfig;
use crate::error::{ConfigError, ProviderError};
use crate::types::{ChatEvent, ChatRequest, Health, LoadedModel, ModelInfo};

/// Event stream of one generation. Ends with `ChatEvent::Done` or with an error.
pub type ChatStream = BoxStream<'static, Result<ChatEvent, ProviderError>>;

/// Contract every model provider fulfils. The TUI and the CLI know only
/// this.
#[async_trait]
pub trait Provider: Send + Sync {
    /// Id of the configured instance: "ollama", "lmstudio", "openrouter"…
    fn id(&self) -> &str;
    /// Implementation kind: "ollama", "openai"…
    fn kind(&self) -> &'static str;
    /// Base URL it talks to, for display.
    fn base_url(&self) -> &str;
    /// Connectivity and version. Called at startup and on `/provider`.
    async fn health(&self) -> Result<Health, ProviderError>;
    /// Models this instance offers.
    async fn list_models(&self) -> Result<Vec<ModelInfo>, ProviderError>;
    /// Details of one model (context, family…). By default looks it up in `list_models`.
    async fn model_info(&self, model: &str) -> Result<ModelInfo, ProviderError> {
        self.list_models()
            .await?
            .into_iter()
            .find(|m| m.id == model)
            .ok_or_else(|| ProviderError::ModelNotFound(model.to_string()))
    }
    /// Models the provider has loaded in memory right now.
    /// `None`: it cannot tell (the OpenAI-compatible API has no equivalent
    /// to `/api/ps`).
    async fn loaded(&self) -> Result<Option<Vec<LoadedModel>>, ProviderError> {
        Ok(None)
    }
    /// Streaming chat. Cancelling the token cuts the stream off with `Cancelled`.
    async fn chat(
        &self,
        req: ChatRequest,
        cancel: CancellationToken,
    ) -> Result<ChatStream, ProviderError>;
}

/// Builds instances of one provider kind from the configuration.
pub trait ProviderFactory: Send + Sync {
    fn kind(&self) -> &'static str;
    fn build(&self, id: &str, cfg: &ProviderConfig) -> Result<Arc<dyn Provider>, ConfigError>;
}
