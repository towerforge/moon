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
    pub tools: ToolsConfig,
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
    /// Ask GitHub once a day whether there is a newer moon and say so at
    /// startup. Off by default: the only traffic moon makes is to the
    /// providers, unless this is turned on. `moon update` checks anyway.
    pub update_check: bool,
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
            update_check: false,
        }
    }
}

/// What the model may do with one capability: nothing, or only with the
/// user's ok each time, or on its own. Everything an agent's permissions
/// panel lists, file tools and commands alike, has one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Permission {
    /// Not offered to the model at all.
    #[default]
    Off,
    /// Shown and waited for: a diff, or a command line, to confirm or skip.
    Ask,
    /// Runs on its own.
    Allow,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::Off => "off",
            Permission::Ask => "ask",
            Permission::Allow => "allow",
        }
    }
}

/// The ids of the file capabilities in the catalogue the permissions panel
/// lists, as they are written in an agent's file. The commands are their
/// own ids: `git diff`, `ls`.
pub mod ids {
    pub const READ_FILES: &str = "read files";
    pub const EDIT_FILES: &str = "edit existing files";
    pub const CREATE_FILES: &str = "create new files";
    pub const SUBFOLDERS: &str = "commands in subfolders";
    /// What `SUBFOLDERS` was called before it left the `Files` group; a
    /// file that still says it means the same.
    pub const SUBFOLDERS_OLD: &str = "run inside subfolders";
}

/// The model acting on the project: the two limits every agent runs under,
/// and four keys from before agents carried their permissions, read once —
/// when `agents/default.toml` is first written — and never again.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct ToolsConfig {
    /// From before agents existed: `enabled` gives `read files` its
    /// `allow`, `edit` and `create` give `edit existing files` and `create
    /// new files` their `ask`. Only what `default.toml` starts from when
    /// there is none yet.
    pub enabled: bool,
    pub edit: bool,
    pub create: bool,
    /// The same, as a table by the ids the panel shows: `"read files" =
    /// "allow"`, `"git diff" = "allow"`. It wins over the three keys.
    pub permissions: BTreeMap<String, Permission>,
    /// Files bigger than this are neither read nor edited.
    pub max_file_bytes: usize,
    /// Paths the model may not touch, as globs relative to the project root,
    /// on top of `.git/` and the secrets moon never attaches.
    pub deny: Vec<String>,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            // a file that only says `enabled = true` keeps meaning what it
            // meant before these two existed: all four tools
            edit: true,
            create: true,
            permissions: BTreeMap::new(),
            max_file_bytes: crate::context::DEFAULT_MAX_BYTES,
            deny: vec![".github/workflows/**".to_string()],
        }
    }
}

impl ToolsConfig {
    /// What the four older keys say: the table, or, while it is empty, what
    /// the three booleans mean. Nothing, from a configuration that never
    /// set them.
    pub fn startup_permissions(&self) -> BTreeMap<String, Permission> {
        if !self.permissions.is_empty() {
            return self.permissions.clone();
        }
        let mut p = BTreeMap::new();
        if self.enabled {
            p.insert(ids::READ_FILES.to_string(), Permission::Allow);
            if self.edit {
                p.insert(ids::EDIT_FILES.to_string(), Permission::Ask);
            }
            if self.create {
                p.insert(ids::CREATE_FILES.to_string(), Permission::Ask);
            }
        }
        p
    }
}

/// Writes `text` as `path`, through a temporary file next to it, the folder
/// made if it was not there: how every file moon writes whole lands, so a
/// crash mid-write leaves the old one.
pub fn write_text(path: &Path, text: &str) -> Result<(), ConfigError> {
    let io = |p: &Path| {
        let p = p.to_path_buf();
        move |source| ConfigError::Io { path: p, source }
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(io(dir))?;
    }
    let tmp = path.with_extension("toml.tmp");
    std::fs::write(&tmp, text).map_err(io(&tmp))?;
    std::fs::rename(&tmp, path).map_err(io(path))
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
    /// There was no file: default values, and where `moon config init` would
    /// write one.
    Default(PathBuf),
    File(PathBuf),
}

impl ConfigSource {
    /// Where the configuration lives, or would once written.
    pub fn path(&self) -> &Path {
        match self {
            ConfigSource::Default(p) | ConfigSource::File(p) => p,
        }
    }
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
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok((
                Self::with_defaults(),
                ConfigSource::Default(path.to_path_buf()),
            )),
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
    fn the_template_is_valid() {
        let cfg = Config::parse(TEMPLATE, Path::new("template")).unwrap();
        assert!(cfg.general.mouse);
        assert!(cfg.general.save_sessions);
        // nothing reaches out to github unless it is asked for
        assert!(!cfg.general.update_check);
        let ollama = &cfg.providers["ollama"];
        assert_eq!(ollama.kind, "ollama");
        assert_eq!(ollama.base_url.as_deref(), Some("http://localhost:11434"));
        assert!(ollama.enabled);
        // the permissions are the agents': the template says nothing about
        // them, so nothing is on until default.toml or the panel says so
        assert!(cfg.tools.startup_permissions().is_empty());
        assert!(TEMPLATE.contains("agents/"));
        // the rest of the block stays commented: the defaults apply
        assert_eq!(
            cfg.tools.max_file_bytes,
            ToolsConfig::default().max_file_bytes
        );
        assert_eq!(cfg.tools.deny, ToolsConfig::default().deny);
        // the table, once there, is what counts
        let with = Config::parse(
            "[tools]\nenabled = true\n[tools.permissions]\n\"git diff\" = \"allow\"\n\"git commit\" = \"ask\"\n",
            Path::new("t"),
        )
        .unwrap();
        let p = with.tools.startup_permissions();
        assert_eq!(p.get("git diff"), Some(&Permission::Allow));
        assert_eq!(p.get("git commit"), Some(&Permission::Ask));
        assert_eq!(p.get(ids::READ_FILES), None);
        // a value that is not one of the three is an error
        assert!(
            Config::parse("[tools.permissions]\n\"ls\" = \"maybe\"\n", Path::new("t")).is_err()
        );
    }

    #[test]
    fn a_file_that_only_enables_tools_still_gets_all_four() {
        // the two scope keys are newer than `enabled`: a configuration
        // written before them must keep meaning what it meant
        let cfg = Config::parse("[tools]\nenabled = true\n", Path::new("t")).unwrap();
        assert_eq!(
            (cfg.tools.enabled, cfg.tools.edit, cfg.tools.create),
            (true, true, true)
        );
        let p = cfg.tools.startup_permissions();
        assert_eq!(p.get(ids::READ_FILES), Some(&Permission::Allow));
        assert_eq!(p.get(ids::EDIT_FILES), Some(&Permission::Ask));
        assert_eq!(p.get(ids::CREATE_FILES), Some(&Permission::Ask));
        assert!(ToolsConfig::default().startup_permissions().is_empty());
    }

    #[test]
    fn extra_and_overrides() {
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
        // tools stay off unless the file says so
        assert!(!cfg.tools.enabled);
        assert_eq!(cfg.tools.deny, vec![".github/workflows/**"]);
        let with_tools = Config::parse(
            "[tools]\nenabled = true\ndeny = [\"secrets/**\"]\n",
            Path::new("t"),
        )
        .unwrap();
        assert!(with_tools.tools.enabled);
        assert_eq!(with_tools.tools.deny, vec!["secrets/**"]);
        assert_eq!(with_tools.tools.max_file_bytes, 200_000);
        // back to TOML and again to Config
        let again = Config::parse(&cfg.to_toml(), Path::new("t2")).unwrap();
        assert_eq!(again, cfg);
    }

    #[test]
    fn without_providers_it_adds_ollama() {
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
    fn write_text_makes_the_folder_and_leaves_no_temporary_behind() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deeper").join("x.toml");
        write_text(&path, "a = 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 1\n");
        assert!(!path.with_extension("toml.tmp").exists());
        write_text(&path, "a = 2\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "a = 2\n");
        assert_eq!(Permission::default(), Permission::Off);
        assert!(Permission::Off < Permission::Ask && Permission::Ask < Permission::Allow);
        assert_eq!(Permission::Allow.as_str(), "allow");
    }

    #[test]
    fn a_missing_file_is_the_default() {
        let (cfg, src) = Config::load_or_default(Path::new("/no/such/config.toml")).unwrap();
        assert_eq!(
            src,
            ConfigSource::Default(PathBuf::from("/no/such/config.toml"))
        );
        assert_eq!(src.path(), Path::new("/no/such/config.toml"));
        assert!(cfg.providers.contains_key("ollama"));
    }
}
