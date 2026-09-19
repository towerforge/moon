use std::path::PathBuf;

use thiserror::Error;

/// Errors returned by a provider. The messages are meant to be shown as-is
/// in the interface.
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("cannot connect to {url}: {detail}")]
    Unreachable { url: String, detail: String },
    #[error("HTTP {status}: {body}")]
    Http { status: u16, body: String },
    #[error("model not found: {0}")]
    ModelNotFound(String),
    #[error("authentication rejected")]
    Auth,
    #[error("invalid response: {0}")]
    Decode(String),
    #[error("cancelled")]
    Cancelled,
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid configuration in {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },
    #[error("provider `{id}`: unknown type `{kind}` (known: {known})")]
    UnknownKind {
        id: String,
        kind: String,
        known: String,
    },
    /// The provider cannot be built with that configuration; not fatal, the
    /// registry leaves it disabled with this reason.
    #[error("provider `{id}`: {detail}")]
    Provider { id: String, detail: String },
    #[error("file already exists: {0}")]
    Exists(PathBuf),
}

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid record in {path}, line {line}: {source}")]
    Parse {
        path: PathBuf,
        line: usize,
        #[source]
        source: serde_json::Error,
    },
    #[error("session not found: {0}")]
    NotFound(String),
}
