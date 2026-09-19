//! Core of moon: the provider contract, the domain types, the configuration,
//! the paths and the session store. It knows nothing about the terminal or
//! any concrete HTTP.

pub mod config;
pub mod context;
pub mod error;
pub mod model_ref;
pub mod params;
pub mod paths;
pub mod provider;
pub mod registry;
pub mod session;
pub mod types;

pub use config::{Config, ConfigSource, GeneralConfig, ProviderConfig, ThemeConfig};
pub use context::{Attachment, Spec};
pub use error::{ConfigError, ProviderError, SessionError};
pub use paths::Paths;
pub use provider::{ChatStream, Provider, ProviderFactory};
pub use registry::Registry;
pub use session::{Session, SessionMeta, SessionStore};
pub use types::{
    Capabilities, ChatEvent, ChatRequest, GenerationParams, Health, LoadedModel, Message,
    ModelInfo, Role, ToolCall, Usage,
};
