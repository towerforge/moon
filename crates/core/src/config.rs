use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::ConfigError;
use crate::types::GenerationParams;

/// Commented template written by `moon config init`.
pub const TEMPLATE: &str = include_str!("../config.template.toml");

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub params: GenerationParams,
    pub providers: BTreeMap<String, ProviderConfig>,
    pub theme: ThemeConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct GeneralConfig {
    pub default_model: Option<String>,
    pub system_prompt: Option<String>,
    /// Mouse capture: the wheel scrolls and the conversation can be selected
    /// by dragging. With it off, text selection is the terminal's own.
    pub mouse: bool,
    pub save_sessions: bool,
    /// Project context file, relative to the start-up directory. Empty: none.
    pub context_file: String,
    /// Maximum size of an attachment; anything above it is truncated.
    pub max_attachment_bytes: usize,
    /// Machine CPU and RAM at the bottom right: every 5 s, every second while the model is working.
    pub system_stats: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_model: None,
            system_prompt: None,
            mouse: true,
            save_sessions: true,
            context_file: crate::context::DEFAULT_CONTEXT_FILE.to_string(),
            max_attachment_bytes: crate::context::DEFAULT_MAX_BYTES,
            system_stats: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
#[serde(default)]
pub struct ThemeConfig {
    /// Moon token → hex color. Keys: night, night-raised, night-line, moon,
    /// moon-soft, ink, ink-muted, on-moon, ok, alert.
    pub overrides: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProviderConfig {
    #[serde(rename = "type")]
    pub kind: String,
    pub base_url: Option<String>,
    /// Name of the environment variable holding the API key. Never the key itself.
    pub api_key_env: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub timeout_secs: Option<u64>,
    /// Keys specific to the provider type (`think`, `keep_alive`…).
    #[serde(flatten)]
    pub extra: toml::Table,
}

fn default_true() -> bool {
    true
}

impl ProviderConfig {
    pub fn new(kind: &str) -> Self {
        Self {
            kind: kind.to_string(),
            base_url: None,
            api_key_env: None,
            enabled: true,
            timeout_secs: None,
            extra: toml::Table::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigSource {
    /// There was no file: default values.
    Default,
    File(PathBuf),
}

impl Config {
    /// Configuration without a file: local Ollama and nothing else.
    pub fn with_defaults() -> Self {
        let mut c = Config::default();
        c.providers
            .insert("ollama".into(), ProviderConfig::new("ollama"));
        c
    }

    pub fn parse(text: &str, path: &Path) -> Result<Self, ConfigError> {
        let mut cfg: Config = toml::from_str(text).map_err(|source| ConfigError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        if cfg.providers.is_empty() {
            cfg.providers
                .insert("ollama".into(), ProviderConfig::new("ollama"));
        }
        Ok(cfg)
    }

    /// Loads the file if it exists; otherwise, the default values.
    pub fn load_or_default(path: &Path) -> Result<(Self, ConfigSource), ConfigError> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok((
                Self::parse(&text, path)?,
                ConfigSource::File(path.to_path_buf()),
            )),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Ok((Self::with_defaults(), ConfigSource::Default))
            }
            Err(source) => Err(ConfigError::Io {
                path: path.to_path_buf(),
                source,
            }),
        }
    }

    /// Writes the commented template. Without `force`, it does not overwrite an existing file.
    pub fn write_template(path: &Path, force: bool) -> Result<(), ConfigError> {
        if path.exists() && !force {
            return Err(ConfigError::Exists(path.to_path_buf()));
        }
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|source| ConfigError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
        }
        std::fs::write(path, TEMPLATE).map_err(|source| ConfigError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn to_toml(&self) -> String {
        toml::to_string_pretty(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn la_plantilla_es_valida() {
        let cfg = Config::parse(TEMPLATE, Path::new("plantilla")).unwrap();
        assert!(cfg.general.mouse);
        assert!(cfg.general.save_sessions);
        let ollama = &cfg.providers["ollama"];
        assert_eq!(ollama.kind, "ollama");
        assert_eq!(ollama.base_url.as_deref(), Some("http://localhost:11434"));
        assert!(ollama.enabled);
    }

    #[test]
    fn extra_y_overrides() {
        let text = r##"
[general]
system_prompt = "Answer briefly."
[params]
temperature = 0.2
mirostat = 2
[providers.ollama]
type = "ollama"
think = true
[providers.lm]
type = "openai"
base_url = "http://localhost:1234/v1"
enabled = false
[theme.overrides]
moon = "#ffffff"
"##;
        let cfg = Config::parse(text, Path::new("t")).unwrap();
        assert_eq!(
            cfg.general.system_prompt.as_deref(),
            Some("Answer briefly.")
        );
        assert_eq!(cfg.params.temperature, Some(0.2));
        assert_eq!(cfg.params.extra["mirostat"], toml::Value::Integer(2));
        assert_eq!(
            cfg.providers["ollama"].extra["think"],
            toml::Value::Boolean(true)
        );
        assert!(!cfg.providers["lm"].enabled);
        assert_eq!(cfg.theme.overrides["moon"], "#ffffff");
        // back to TOML and again to Config
        let again = Config::parse(&cfg.to_toml(), Path::new("t2")).unwrap();
        assert_eq!(again, cfg);
    }

    #[test]
    fn sin_proveedores_pone_ollama() {
        // a setting that no longer exists is ignored, not an error: an old
        // file keeps working
        let cfg = Config::parse(
            "[general]\nmouse = false\ntheme = \"auto\"\n",
            Path::new("t"),
        )
        .unwrap();
        assert!(cfg.providers.contains_key("ollama"));
        assert!(!cfg.general.mouse);
    }

    #[test]
    fn fichero_inexistente_es_default() {
        let (cfg, src) = Config::load_or_default(Path::new("/no/existe/config.toml")).unwrap();
        assert_eq!(src, ConfigSource::Default);
        assert!(cfg.providers.contains_key("ollama"));
    }
}
