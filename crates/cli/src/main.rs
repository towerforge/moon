//! The `moon` binary: wires up providers, loads configuration and starts the
//! TUI or a subcommand.

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, bail, Context};
use clap::{Parser, Subcommand};
use futures_util::StreamExt;
use moon_core::context::{self, Spec};
use moon_core::{
    ChatEvent, ChatRequest, Config, ConfigSource, Message, Paths, Registry, SessionStore,
};
use tokio_util::sync::CancellationToken;

#[derive(Parser)]
#[command(
    name = "moon",
    version,
    about = "chat with language models in the terminal"
)]
struct Cli {
    /// Model: provider/model, or just the model when unambiguous
    #[arg(short, long, global = true, value_name = "MODEL")]
    model: Option<String>,
    /// Configuration file (default ~/.config/moon/config.toml)
    #[arg(long, global = true, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Resume the last conversation, or the one with the given id
    #[arg(long, value_name = "ID", num_args = 0..=1, default_missing_value = "latest")]
    resume: Option<String>,
    #[command(subcommand)]
    cmd: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Ask without the UI: the reply streams to stdout
    Ask {
        /// Prompt text; read from stdin when missing
        prompt: Vec<String>,
        /// System prompt for this question
        #[arg(long)]
        system: Option<String>,
        /// Token counts and speed at the end, on stderr
        #[arg(long)]
        stats: bool,
    },
    /// Models of every provider
    Models,
    /// Provider status
    Providers,
    /// Configuration file
    Config {
        #[command(subcommand)]
        action: ConfigCmd,
    },
    /// Saved conversations
    Sessions {
        #[command(subcommand)]
        action: SessionsCmd,
    },
}

#[derive(Subcommand)]
enum ConfigCmd {
    /// Write a commented configuration file
    Init {
        /// Overwrite if it already exists
        #[arg(long)]
        force: bool,
    },
    /// File path
    Path,
    /// Effective configuration
    Show,
}

#[derive(Subcommand)]
enum SessionsCmd {
    /// List saved conversations
    List,
}

fn init_logging(paths: &Paths) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    use tracing_subscriber::EnvFilter;
    std::fs::create_dir_all(&paths.state_dir).ok()?;
    let file = tracing_appender::rolling::never(&paths.state_dir, "moon.log");
    let (writer, guard) = tracing_appender::non_blocking(file);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(writer)
        .with_ansi(false)
        .init();
    Some(guard)
}

fn build_registry(cfg: &Config) -> anyhow::Result<Registry> {
    let mut registry = Registry::new();
    registry.register(Box::new(moon_provider_ollama::Factory));
    registry.register(Box::new(moon_provider_openai::Factory));
    registry.build_all(cfg)?;
    Ok(registry)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let paths = Paths::from_env();
    let _log_guard = init_logging(&paths);
    let config_path = cli.config.clone().unwrap_or_else(|| paths.config_file());

    if let Some(Cmd::Config { action }) = &cli.cmd {
        return config_cmd(action, &config_path, &paths);
    }

    let (cfg, source) = Config::load_or_default(&config_path)?;
    let registry = Arc::new(build_registry(&cfg)?);
    let store = if cfg.general.save_sessions {
        Some(SessionStore::new(paths.sessions_dir()))
    } else {
        None
    };

    match cli.cmd {
        None => {
            let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let cwd = shorten_home(&root);
            moon_tui::run(moon_tui::RunOptions {
                config: cfg,
                config_source: source,
                registry,
                store,
                resume: cli.resume,
                model: cli.model,
                version: env!("CARGO_PKG_VERSION").to_string(),
                cwd,
                root,
                state_dir: Some(paths.state_dir.clone()),
            })
            .await
        }
        Some(Cmd::Ask {
            prompt,
            system,
            stats,
        }) => ask(&cfg, &registry, cli.model, prompt, system, stats).await,
        Some(Cmd::Models) => models(&registry).await,
        Some(Cmd::Providers) => providers(&registry).await,
        Some(Cmd::Sessions {
            action: SessionsCmd::List,
        }) => sessions_list(store.as_ref(), &paths),
        Some(Cmd::Config { .. }) => unreachable!("handled earlier"),
    }
}

fn shorten_home(p: &std::path::Path) -> String {
    let s = p.display().to_string();
    match std::env::var("HOME") {
        Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]),
        _ => s,
    }
}

fn config_cmd(action: &ConfigCmd, path: &std::path::Path, paths: &Paths) -> anyhow::Result<()> {
    match action {
        ConfigCmd::Init { force } => {
            Config::write_template(path, *force)
                .map_err(|e| anyhow!("{e} (use --force to overwrite)"))?;
            println!("configuration written to {}", path.display());
            Ok(())
        }
        ConfigCmd::Path => {
            println!("{}", path.display());
            println!("sessions: {}", paths.sessions_dir().display());
            println!("log:      {}", paths.log_file().display());
            Ok(())
        }
        ConfigCmd::Show => {
            let (cfg, source) = Config::load_or_default(path)?;
            match source {
                ConfigSource::Default => {
                    println!("# no file at {}: defaults", path.display())
                }
                ConfigSource::File(p) => println!("# {}", p.display()),
            }
            print!("{}", cfg.to_toml());
            Ok(())
        }
    }
}

async fn pick_model(
    cfg: &Config,
    registry: &Registry,
    explicit: Option<String>,
) -> anyhow::Result<(Arc<dyn moon_core::Provider>, String)> {
    if registry.is_empty() {
        bail!("no provider available: check the configuration");
    }
    if let Some(spec) = explicit.or_else(|| cfg.general.default_model.clone()) {
        return registry.resolve(&spec, None).map_err(|e| anyhow!(e));
    }
    for (id, p) in registry.providers() {
        if let Ok(models) = p.list_models().await {
            if let Some(m) = models.first() {
                return Ok((p.clone(), m.id.clone()));
            }
        }
        tracing::debug!(provider = %id, "no models");
    }
    bail!("no provider has models: is Ollama running?")
}

async fn ask(
    cfg: &Config,
    registry: &Registry,
    model: Option<String>,
    prompt: Vec<String>,
    system: Option<String>,
    stats: bool,
) -> anyhow::Result<()> {
    let mut text = prompt.join(" ");
    if text.trim().is_empty() {
        if std::io::stdin().is_terminal() {
            bail!("pass the prompt as an argument or on stdin");
        }
        std::io::stdin()
            .read_to_string(&mut text)
            .context("reading stdin")?;
    }
    let text = text.trim().to_string();
    if text.is_empty() {
        bail!("the prompt is empty");
    }
    let (provider, model) = pick_model(cfg, registry, model).await?;
    // same context as the TUI: MOON.md and @path mentions
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut attachments = Vec::new();
    for raw in moon_tui::mentions::extract(&text) {
        let spec = Spec::parse(&raw).map_err(|e| anyhow!("@{raw}: {e}"))?;
        let a = context::read_attachment(&root, &spec, cfg.general.max_attachment_bytes)
            .map_err(|e| anyhow!("@{raw}: {e}"))?;
        attachments.push(a);
    }
    let context_file = context::load_context_file(&root, &cfg.general.context_file)?;
    let base = system.or_else(|| cfg.general.system_prompt.clone());
    let mut messages = Vec::new();
    if let Some(sp) = context::build_system_prompt(
        base.as_deref(),
        context_file
            .as_ref()
            .map(|c| (cfg.general.context_file.as_str(), c.as_str())),
        &[],
    ) {
        messages.push(Message::system(sp));
    }
    let mut user = Message::user(text);
    user.attachments = attachments;
    messages.push(user);
    let req = ChatRequest {
        model: model.clone(),
        messages,
        params: cfg.params.clone(),
    };
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            c2.cancel();
        }
    });
    let mut stream = provider
        .chat(req, cancel)
        .await
        .map_err(|e| anyhow!("{}/{}: {e}", provider.id(), model))?;
    let mut out = std::io::stdout();
    let mut printed = false;
    while let Some(ev) = stream.next().await {
        match ev {
            Ok(ChatEvent::Delta(d)) => {
                out.write_all(d.as_bytes())?;
                out.flush()?;
                printed = true;
            }
            Ok(ChatEvent::Thinking(_)) | Ok(ChatEvent::ToolCall(_)) => {}
            Ok(ChatEvent::Done(u)) => {
                if printed {
                    println!();
                }
                if stats {
                    eprintln!(
                        "{}/{} · {} prompt tokens · {} generated · {:.1} tok/s",
                        provider.id(),
                        model,
                        u.prompt_tokens.unwrap_or(0),
                        u.completion_tokens.unwrap_or(0),
                        u.tokens_per_second.unwrap_or(0.0)
                    );
                }
                return Ok(());
            }
            Err(moon_core::ProviderError::Cancelled) => {
                if printed {
                    println!();
                }
                eprintln!("cancelled");
                std::process::exit(130);
            }
            Err(e) => {
                if printed {
                    println!();
                }
                return Err(anyhow!("{e}"));
            }
        }
    }
    if printed {
        println!();
    }
    Ok(())
}

async fn models(registry: &Registry) -> anyhow::Result<()> {
    if registry.is_empty() {
        bail!("no provider available");
    }
    let mut rows: Vec<[String; 5]> = Vec::new();
    let mut errors = Vec::new();
    for (id, res) in registry.list_all_models().await {
        match res {
            Ok(list) => {
                for m in list {
                    rows.push([
                        m.qualified(),
                        m.size_bytes
                            .map(moon_tui::app::fmt_size)
                            .unwrap_or_default(),
                        m.parameter_size.unwrap_or_default(),
                        m.quantization.unwrap_or_default(),
                        m.family.unwrap_or_default(),
                    ]);
                }
            }
            Err(e) => errors.push(format!("✗ {id}: {e}")),
        }
    }
    if !rows.is_empty() {
        let w = rows
            .iter()
            .map(|r| r[0].chars().count())
            .max()
            .unwrap_or(10);
        println!(
            "{:w$}  {:>8}  {:>7}  {:8}  family",
            "model",
            "size",
            "params",
            "quant.",
            w = w
        );
        for r in &rows {
            println!(
                "{:w$}  {:>8}  {:>7}  {:8}  {}",
                r[0],
                r[1],
                r[2],
                r[3],
                r[4],
                w = w
            );
        }
    }
    for e in &errors {
        eprintln!("{e}");
    }
    for (id, why) in registry.disabled() {
        eprintln!("– {id}: disabled ({why})");
    }
    if rows.is_empty() && !errors.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

async fn providers(registry: &Registry) -> anyhow::Result<()> {
    let mut any_ok = false;
    for (id, p) in registry.providers() {
        match p.health().await {
            Ok(h) => {
                any_ok = true;
                let mut extra = String::new();
                if let Some(v) = h.version {
                    extra.push_str(&format!(" · v{v}"));
                }
                if let Some(d) = h.detail {
                    extra.push_str(&format!(" · {d}"));
                }
                println!("● {id} · {} · {} · ok{extra}", p.kind(), p.base_url());
            }
            Err(e) => println!("✗ {id} · {} · {} · {e}", p.kind(), p.base_url()),
        }
    }
    for (id, why) in registry.disabled() {
        println!("– {id} · disabled: {why}");
    }
    if registry.is_empty() {
        println!("no providers configured");
    }
    if !any_ok && !registry.is_empty() {
        std::process::exit(1);
    }
    Ok(())
}

fn sessions_list(store: Option<&SessionStore>, paths: &Paths) -> anyhow::Result<()> {
    let Some(store) = store else {
        bail!("sessions are disabled (save_sessions = false)");
    };
    let list = store.list()?;
    if list.is_empty() {
        println!("no sessions in {}", paths.sessions_dir().display());
        return Ok(());
    }
    for m in list {
        println!(
            "{}  {}  {:28}  {}",
            m.id,
            m.created_at
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M"),
            m.model.unwrap_or_default(),
            m.title
        );
    }
    Ok(())
}
