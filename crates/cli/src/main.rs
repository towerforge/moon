//! The `moon` binary: wires up providers, loads configuration and starts the
//! TUI or a subcommand.

use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

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
    /// Update moon: checks GitHub and installs the new release
    Update {
        /// Say what there is and install nothing
        #[arg(long)]
        check: bool,
        /// Install without asking
        #[arg(long, short = 'y')]
        yes: bool,
        /// Install over a cargo install or a build under target/, and
        /// reinstall a version that is already there
        #[arg(long)]
        force: bool,
        /// A specific version instead of the latest one
        #[arg(long, value_name = "VERSION")]
        to: Option<String>,
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
    // updating needs no providers and must work with a broken configuration
    if let Some(Cmd::Update {
        check,
        yes,
        force,
        to,
    }) = &cli.cmd
    {
        return update_cmd(*check, *yes, *force, to.as_deref(), &config_path, &paths).await;
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
        Some(Cmd::Config { .. }) | Some(Cmd::Update { .. }) => unreachable!("handled earlier"),
    }
}

/// `moon update`. The panel is moon's own, so it takes the theme from the
/// configuration; an unreadable configuration is no reason not to update.
async fn update_cmd(
    check: bool,
    yes: bool,
    force: bool,
    to: Option<&str>,
    config_path: &std::path::Path,
    paths: &Paths,
) -> anyhow::Result<()> {
    let to = to.map(str::parse::<moon_updater::Version>).transpose()?;
    let overrides = Config::load_or_default(config_path)
        .map(|(cfg, _)| cfg.theme.overrides)
        .unwrap_or_default();
    let outcome = moon_tui::update::run(moon_tui::update::Options {
        current: env!("CARGO_PKG_VERSION").parse()?,
        to,
        check_only: check,
        yes,
        force,
        state_dir: Some(paths.state_dir.clone()),
        theme: moon_tui::Theme::resolve(&overrides).0,
    })
    .await?;
    // what went wrong has already been said, in the panel or on stderr
    if outcome == moon_tui::update::Outcome::Failed {
        std::process::exit(1);
    }
    Ok(())
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
                ConfigSource::Default(p) => println!("# no file at {}: defaults", p.display()),
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

/// The star of the TUI, for the wait of `moon ask`: nothing has come back yet
/// and a local model can take seconds to load. It turns on **stderr**, and
/// only when stderr is a terminal, so a pipe still gets the reply and nothing
/// else. Erased before the first token is printed.
struct Waiting {
    start: std::time::Instant,
    frame: usize,
    /// stderr is a terminal: there is someone watching.
    on: bool,
    /// …and colour was not turned off.
    colour: bool,
    drawn: bool,
    /// The model has sent reasoning: it is thinking, not loading.
    thinking: bool,
}

impl Waiting {
    fn new() -> Self {
        let on = std::io::stderr().is_terminal();
        Self {
            start: std::time::Instant::now(),
            frame: 0,
            on,
            colour: on && std::env::var_os("NO_COLOR").is_none(),
            drawn: false,
            thinking: false,
        }
    }

    /// `moon` for the star, `ink-muted` for the words, as in the TUI. Empty
    /// when the terminal says nothing about colour.
    fn paint(&self, token: &str) -> String {
        if !self.colour {
            return String::new();
        }
        let Some((_, hex, idx)) = moon_tui::theme::TOKENS.iter().find(|(n, _, _)| *n == token)
        else {
            return String::new();
        };
        if !moon_tui::Theme::truecolor_supported() {
            return format!("\x1b[38;5;{idx}m");
        }
        match u32::from_str_radix(hex.trim_start_matches('#'), 16) {
            Ok(v) => format!(
                "\x1b[38;2;{};{};{}m",
                (v >> 16) & 0xff,
                (v >> 8) & 0xff,
                v & 0xff
            ),
            Err(_) => String::new(),
        }
    }

    fn tick(&mut self) {
        if !self.on {
            return;
        }
        let frames = moon_tui::app::SPINNER;
        let star = frames[self.frame % frames.len()];
        self.frame += 1;
        // the same two words the TUI uses while it waits for the first
        // token, except that here reasoning tokens settle which one it is
        // instead of the 1.5 s guess
        let verb = match self.start.elapsed() > Duration::from_millis(1500) && !self.thinking {
            true => "loading model…",
            false => "thinking…",
        };
        let (moon, muted) = (self.paint("moon"), self.paint("ink-muted"));
        let off = if self.colour { "\x1b[0m" } else { "" };
        eprint!(
            "\r\x1b[2K{moon}{star}{off} {muted}{verb} ({}){off}",
            moon_tui::app::fmt_dur(self.start.elapsed())
        );
        let _ = std::io::stderr().flush();
        self.drawn = true;
    }

    /// Takes the line back before anything else is written.
    fn clear(&mut self) {
        if self.on && self.drawn {
            eprint!("\r\x1b[2K");
            let _ = std::io::stderr().flush();
            self.drawn = false;
        }
    }
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
    let piped = text.trim().is_empty();
    if piped {
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
    // same context as the TUI: MOON.md and @path mentions. Only in what was
    // typed: what comes down a pipe is content, and a diff or a log is full
    // of `@@` and `@Annotation` tokens that are not paths
    let root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut attachments = Vec::new();
    if !piped {
        for raw in moon_tui::mentions::extract(&text) {
            let spec = Spec::parse(&raw).map_err(|e| anyhow!("@{raw}: {e}"))?;
            let a = context::read_attachment(&root, &spec, cfg.general.max_attachment_bytes)
                .map_err(|e| anyhow!("@{raw}: {e}"))?;
            attachments.push(a);
        }
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
        tools: Vec::new(),
    };
    let cancel = CancellationToken::new();
    let c2 = cancel.clone();
    tokio::spawn(async move {
        if tokio::signal::ctrl_c().await.is_ok() {
            c2.cancel();
        }
    });
    let mut waiting = Waiting::new();
    // a first turn late enough that a quick answer never makes the star blink
    let mut ticker = tokio::time::interval_at(
        tokio::time::Instant::now() + Duration::from_millis(250),
        moon_tui::app::TICK,
    );
    // the star has to turn here too, not only over the stream: with a model
    // that is not in memory, Ollama holds the response until it has loaded
    // it, and that await is most of the silence
    let chat = provider.chat(req, cancel);
    tokio::pin!(chat);
    let mut stream = loop {
        tokio::select! {
            opened = &mut chat => match opened {
                Ok(s) => break s,
                Err(e) => {
                    waiting.clear();
                    return Err(anyhow!("{}/{}: {e}", provider.id(), model));
                }
            },
            _ = ticker.tick() => waiting.tick(),
        }
    };
    let mut out = std::io::stdout();
    let mut printed = false;
    loop {
        let ev = tokio::select! {
            ev = stream.next() => match ev {
                Some(ev) => ev,
                None => break,
            },
            _ = ticker.tick(), if !printed => {
                waiting.tick();
                continue;
            }
        };
        match ev {
            Ok(ChatEvent::Delta(d)) => {
                waiting.clear();
                out.write_all(d.as_bytes())?;
                out.flush()?;
                printed = true;
            }
            // the reasoning itself is not printed: it is not the answer, and
            // stdout belongs to whoever is reading it
            Ok(ChatEvent::Thinking(_)) => waiting.thinking = true,
            Ok(ChatEvent::ToolCall(_)) => {}
            Ok(ChatEvent::Done(u)) => {
                waiting.clear();
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
                waiting.clear();
                if printed {
                    println!();
                }
                eprintln!("cancelled");
                std::process::exit(130);
            }
            Err(e) => {
                waiting.clear();
                if printed {
                    println!();
                }
                return Err(anyhow!("{e}"));
            }
        }
    }
    waiting.clear();
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
