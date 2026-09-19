//! Application state and logic, Elm style: everything that happens is an
//! `Action`, `update` applies it and `view` (in `view.rs`) paints the state.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use moon_core::session::{export_markdown, title_from};
use moon_core::{
    params, ChatEvent, ChatRequest, Config, ConfigSource, GenerationParams, Health, LoadedModel,
    Message, ModelInfo, ProviderError, Registry, Role, Session, SessionMeta, SessionStore, Usage,
};
use ratatui::text::{Line, Span};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::commands::{self, Command};
use crate::logo::{logo_rows, LOGO_COLS, LOGO_PAD};
use crate::markdown::Renderer;
use crate::mentions;
use crate::picker::{Picker, PickerGroup, PickerItem};
use crate::theme::Theme;
use crate::wrap::{pad_line, wrap_line};
use moon_core::context::{self, Attachment, Spec};

mod chat;
mod files;
mod keys;
mod models;
mod render;
mod slash;
mod status;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tests_context;

pub type Tx = mpsc::UnboundedSender<Action>;

#[derive(Debug)]
pub enum Action {
    Key(KeyEvent),
    Paste(String),
    Resize,
    Tick,
    ScrollBy(i32),
    /// Left button pressed at (column, row).
    MouseDown(u16, u16),
    /// Drag with the left button.
    MouseDrag(u16, u16),
    /// Left button released.
    MouseUp(u16, u16),
    ProviderChecked(String, Result<Health, String>),
    ModelsLoaded(String, Result<Vec<ModelInfo>, String>),
    ModelsDone,
    ModelInfo(ModelInfo),
    Stream(u64, StreamEvent),
    SessionLoaded(Box<Session>),
    Notice(String),
    /// CPU and RAM reading from the `sysmon` thread.
    SysSample(crate::sysmon::Sample),
    /// Time to ask the provider whether it has the model loaded.
    PollLoaded,
    /// Answer to that question, for the model that was asked about.
    Loaded(Current, LoadedState),
    Quit,
}

#[derive(Debug)]
pub enum StreamEvent {
    Delta(String),
    Thinking(String),
    Done(Usage),
    Cancelled,
    Error(String),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Item {
    Message(Message),
    Error(String),
    Info(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Current {
    pub provider: String,
    pub model: String,
}

/// Whether the provider has the active model in memory.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum LoadedState {
    /// Not asked yet, or the last query failed: worth asking again.
    #[default]
    Unknown,
    /// The provider cannot tell (no `/api/ps` equivalent): stop asking until
    /// the model changes.
    Unsupported,
    NotLoaded,
    Loaded(LoadedModel),
}

impl Current {
    pub fn qualified(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }
}

pub enum Generation {
    Idle,
    Streaming {
        cancel: CancellationToken,
        started: Instant,
        first_at: Option<Instant>,
        deltas: u32,
        /// Estimated tokens of the prompt sent: the real count only arrives at the end.
        sent: u32,
    },
}

/// Summary of the last generation, shown on the left of the status line
/// until the next request starts.
#[derive(Clone, Debug, PartialEq)]
pub struct RunSummary {
    pub elapsed: Duration,
    /// Prompt tokens: the real ones if the provider gave them, else the estimate.
    pub sent: u32,
    pub sent_estimated: bool,
    pub received: u32,
    pub tok_per_s: Option<f32>,
    pub cancelled: bool,
}

/// What the bottom panel is showing. Only one at a time, and while there is
/// one the input box gives it its place.
pub enum Panel {
    Models(Picker),
    Sessions(Picker),
    /// What is attached to every request: the files, the project context file
    /// and the two buttons that add and detach.
    Files(Picker),
    /// Walking the project tree to attach something; `dir` is where it stands,
    /// relative to the root.
    Browse {
        picker: Box<Picker>,
        dir: PathBuf,
    },
    Help(HelpState),
    /// What is being decided about a session, in the sessions list's place;
    /// the list is kept so we can return to it as it was.
    SessionAction {
        picker: Box<Picker>,
        action: SessionAction,
    },
}

impl Panel {
    /// The list the panel is showing, whichever it is. The help and the
    /// session dialogs have none.
    pub fn picker(&self) -> Option<&Picker> {
        match self {
            Panel::Models(p) | Panel::Sessions(p) | Panel::Files(p) => Some(p),
            Panel::Browse { picker, .. } => Some(picker),
            Panel::Help(_) | Panel::SessionAction { .. } => None,
        }
    }

    pub fn picker_mut(&mut self) -> Option<&mut Picker> {
        match self {
            Panel::Models(p) | Panel::Sessions(p) | Panel::Files(p) => Some(p),
            Panel::Browse { picker, .. } => Some(picker),
            Panel::Help(_) | Panel::SessionAction { .. } => None,
        }
    }
}

/// What is being done with a session from the list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionAction {
    Delete {
        id: String,
        title: String,
        choice: Choice,
    },
    Rename {
        id: String,
        /// New title, as it is being typed.
        input: String,
    },
}

/// Highlighted button in the delete dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    Delete,
    Keep,
}

impl Choice {
    fn toggle(self) -> Self {
        match self {
            Choice::Delete => Choice::Keep,
            Choice::Keep => Choice::Delete,
        }
    }
}

/// Sections of the help. They are the tabs of the title row and tab walks
/// them; each one is read from the top.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum HelpTab {
    /// What moon is and the handful of keys to get going.
    #[default]
    General,
    Commands,
    Keys,
}

impl HelpTab {
    pub const ALL: [HelpTab; 3] = [HelpTab::General, HelpTab::Commands, HelpTab::Keys];

    pub fn title(self) -> &'static str {
        match self {
            HelpTab::General => "General",
            HelpTab::Commands => "Commands",
            HelpTab::Keys => "Keys",
        }
    }

    /// The tab `delta` steps away, wrapping around at both ends.
    pub fn shift(self, delta: i32) -> Self {
        let n = Self::ALL.len() as i32;
        let i = Self::ALL.iter().position(|t| *t == self).unwrap_or(0) as i32;
        Self::ALL[(i + delta).rem_euclid(n) as usize]
    }
}

/// Open section of the help and its scroll. `rows` and `total` are left by the
/// last paint to clamp it.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct HelpState {
    pub tab: HelpTab,
    pub scroll: usize,
    pub rows: usize,
    pub total: usize,
}

impl HelpState {
    /// Another section: it opens at the top, not where the last one was left.
    pub fn select(&mut self, tab: HelpTab) {
        self.tab = tab;
        self.scroll = 0;
    }

    pub fn scroll_by(&mut self, delta: i32) {
        let max = self.total.saturating_sub(self.rows) as i64;
        self.scroll = (self.scroll as i64 + delta as i64).clamp(0, max) as usize;
    }

    pub fn page(&self) -> i32 {
        self.rows.saturating_sub(1).max(1) as i32
    }
}

/// Mouse selection, in document coordinates (row of the full conversation,
/// column), so that it survives scrolling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub anchor: (usize, usize),
    pub head: (usize, usize),
    pub dragging: bool,
}

impl Selection {
    /// (start, end), ordered.
    pub fn bounds(&self) -> ((usize, usize), (usize, usize)) {
        if self.anchor <= self.head {
            (self.anchor, self.head)
        } else {
            (self.head, self.anchor)
        }
    }
}

pub struct ProviderState {
    pub id: String,
    pub kind: &'static str,
    pub base_url: String,
    pub health: Option<Result<Health, String>>,
}

struct Rendered {
    width: u16,
    key: usize,
    lines: Vec<Line<'static>>,
}

pub struct RunOptions {
    pub config: Config,
    pub config_source: ConfigSource,
    pub registry: Arc<Registry>,
    pub store: Option<SessionStore>,
    pub resume: Option<String>,
    pub model: Option<String>,
    pub version: String,
    pub cwd: String,
    /// Real startup directory: root for `@` and for the files panel.
    pub root: PathBuf,
    /// State kept between runs (recent models and sessions). `None` saves nothing.
    pub state_dir: Option<PathBuf>,
}

/// Recent models and sessions shown on top of their lists, and the files
/// where they persist between runs.
const RECENT_MAX: usize = 5;
const RECENT_MODELS_FILE: &str = "recent_models";
const RECENT_SESSIONS_FILE: &str = "recent_sessions";
const RECENT_GROUP: &str = "Recent";
/// Below this many models the whole list is in view: a `Recent` section would
/// only say twice what is already there.
const RECENT_MIN_MODELS: usize = 10;
const ALL_GROUP: &str = "All";
/// Sections of the files panel and of the tree it browses.
const ATTACHED_GROUP: &str = "Attached";
const PROJECT_GROUP: &str = "Project";
const ACTIONS_GROUP: &str = "Actions";
const FOLDERS_GROUP: &str = "Folders";
const FILES_GROUP: &str = "Files";
/// Ids of the rows of the files panel that are not a file: the two buttons
/// and the project context file, which is not attached by hand.
const ADD_ROW: &str = "·add";
const CLEAR_ROW: &str = "·clear";
const CONTEXT_ROW: &str = "·context";

const NOTICE_TTL: Duration = Duration::from_secs(5);
const CTRL_C_WINDOW: Duration = Duration::from_secs(2);
/// Cadence of the activity animation: star and shimmer advance one step per
/// tick. The loop only arms the tick while there is something to animate.
pub const TICK: Duration = Duration::from_millis(120);
/// Minimum width of the hints row for the readings with peak.
const STATS_FULL_MIN_WIDTH: u16 = 120;
/// Minimum width for the readings without peak; below it they are not drawn.
const STATS_SHORT_MIN_WIDTH: u16 = 90;
/// From here on the machine is under strain and the value is highlighted.
const STATS_HOT_PCT: f32 = 90.0;
/// Star that grows and shrinks, one frame per tick.
pub const SPINNER: [&str; 10] = ["·", "✢", "✳", "✶", "✻", "✽", "✻", "✶", "✳", "✢"];
/// Rest ticks between two passes of the shimmer over the activity verb.
const SHIMMER_REST: usize = 6;

pub struct App {
    pub cfg: Config,
    pub theme: Theme,
    registry: Arc<Registry>,
    store: Option<SessionStore>,
    pub version: String,
    pub cwd: String,
    root: PathBuf,
    default_config: bool,
    /// Live attachments from the files panel: re-read on every send.
    pub(crate) live: Vec<Spec>,
    /// Project context file (name, contents), if any.
    pub(crate) context_file: Option<(String, String)>,
    pub session: Option<SessionMeta>,
    pub items: Vec<Item>,
    /// `/clear`: earlier items are not shown, but they stay in the context.
    view_from: usize,
    pub system_prompt: Option<String>,
    pub params: GenerationParams,
    pub current: Option<Current>,
    default_provider: Option<String>,
    pub models: Vec<ModelInfo>,
    pub providers: Vec<ProviderState>,
    pub ctx_len: Option<u32>,
    /// Last chosen models, qualified id, most recent first.
    pub recent: Vec<String>,
    recent_file: Option<PathBuf>,
    /// Last sessions opened or written to, by id, most recent first.
    pub recent_sessions: Vec<String>,
    recent_sessions_file: Option<PathBuf>,
    pub last_usage: Option<Usage>,
    pub last_run: Option<RunSummary>,
    pub loading: bool,

    pub input: crate::input::ChatInput,
    /// Highlighted command suggestion and the prefix it belongs to.
    suggest_sel: usize,
    suggest_for: String,
    history: Vec<String>,
    hist_idx: Option<usize>,
    draft: String,

    pub gen: Generation,
    gen_id: u64,
    pub scroll_offset: usize,
    pub follow: bool,
    pub total_lines: usize,
    pub view_height: usize,
    pub panel: Option<Panel>,
    /// Where the "jump to end" indicator was drawn, for the click.
    pub jump_rect: Option<ratatui::layout::Rect>,
    /// Conversation area in the last paint, for the mouse.
    pub conv_area: Option<ratatui::layout::Rect>,
    /// Input box area in the last paint: a click moves the cursor.
    pub input_area: Option<ratatui::layout::Rect>,
    pub selection: Option<Selection>,
    pub notice: Option<(String, Instant)>,
    pub should_quit: bool,
    ctrl_c_at: Option<Instant>,
    pub spinner: usize,
    /// Latest machine readings, for the hints row and `/context`.
    pub sys: crate::sysmon::History,
    /// Sampling pace: fast while the model is working.
    sys_pace: crate::sysmon::Pace,
    /// Whether the provider has the active model loaded, and how much it takes.
    pub loaded: LoadedState,

    md: Renderer,
    cache: Vec<Option<Rendered>>,
}

fn common_prefix(items: &[String]) -> String {
    let Some(first) = items.first() else {
        return String::new();
    };
    let mut end = first.len();
    for it in &items[1..] {
        end = first
            .char_indices()
            .zip(it.chars())
            .take_while(|((_, a), b)| a == b)
            .map(|((i, a), _)| i + a.len_utf8())
            .last()
            .unwrap_or(0)
            .min(end);
    }
    first[..end].to_string()
}

/// Readable duration for the activity line: `12s`, `1m 20s`, `1h 2m`.
pub fn fmt_dur(d: Duration) -> String {
    let secs = d.as_secs();
    match (secs / 3600, (secs % 3600) / 60, secs % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m {s}s"),
        (h, m, _) => format!("{h}h {m}m"),
    }
}

pub fn fmt_k(n: u32) -> String {
    if n >= 1000 {
        let k = n as f32 / 1000.0;
        if k >= 100.0 {
            format!("{k:.0}k")
        } else {
            format!("{k:.1}k").replace(".0k", "k")
        }
    } else {
        n.to_string()
    }
}

pub fn fmt_size(bytes: u64) -> String {
    let gb = bytes as f64 / 1e9;
    if gb >= 1.0 {
        format!("{gb:.1} GB")
    } else {
        format!("{:.0} MB", bytes as f64 / 1e6)
    }
}

impl App {
    pub fn new(opts: RunOptions) -> Self {
        let recent_file = opts.state_dir.as_ref().map(|d| d.join(RECENT_MODELS_FILE));
        let recent_sessions_file = opts
            .state_dir
            .as_ref()
            .map(|d| d.join(RECENT_SESSIONS_FILE));
        let (theme, warnings) = Theme::resolve(&opts.config.theme.overrides);
        let providers = opts
            .registry
            .providers()
            .map(|(id, p)| ProviderState {
                id: id.clone(),
                kind: p.kind(),
                base_url: p.base_url().to_string(),
                health: None,
            })
            .collect();
        let mut app = Self {
            theme,
            registry: opts.registry,
            store: opts.store,
            version: opts.version,
            cwd: opts.cwd,
            root: opts.root,
            live: Vec::new(),
            context_file: None,
            default_config: opts.config_source == ConfigSource::Default,
            session: None,
            items: Vec::new(),
            view_from: 0,
            system_prompt: opts.config.general.system_prompt.clone(),
            params: opts.config.params.clone(),
            current: None,
            default_provider: None,
            models: Vec::new(),
            providers,
            ctx_len: None,
            recent: models::load_recent(recent_file.as_deref()),
            recent_file,
            recent_sessions: models::load_recent(recent_sessions_file.as_deref()),
            recent_sessions_file,
            last_usage: None,
            last_run: None,
            loading: true,
            input: crate::input::ChatInput::new(),
            suggest_sel: 0,
            suggest_for: String::new(),
            history: Vec::new(),
            hist_idx: None,
            draft: String::new(),
            gen: Generation::Idle,
            gen_id: 0,
            scroll_offset: 0,
            follow: true,
            total_lines: 0,
            view_height: 0,
            panel: None,
            jump_rect: None,
            conv_area: None,
            input_area: None,
            selection: None,
            notice: None,
            should_quit: false,
            ctrl_c_at: None,
            spinner: 0,
            sys: crate::sysmon::History::default(),
            sys_pace: crate::sysmon::Pace::default(),
            loaded: LoadedState::default(),
            md: Renderer::new(),
            cache: Vec::new(),
            cfg: opts.config,
        };
        for w in warnings {
            app.items.push(Item::Info(w));
        }
        for (id, why) in app.registry.disabled() {
            app.items
                .push(Item::Info(format!("provider {id} disabled: {why}")));
        }
        if app.registry.is_empty() {
            app.items.push(Item::Error(
                "no provider available: check the configuration".into(),
            ));
        }
        if let Some(spec) = opts.model.or_else(|| app.cfg.general.default_model.clone()) {
            match app.registry.resolve(&spec, None) {
                Ok((p, m)) => {
                    app.current = Some(Current {
                        provider: p.id().to_string(),
                        model: m,
                    })
                }
                Err(e) => app.items.push(Item::Error(e)),
            }
        }
        app.load_context_file();
        if let Some(id) = opts.resume {
            app.resume(&id);
        }
        app.cache.resize_with(app.items.len(), || None);
        app
    }

    fn resume(&mut self, id: &str) {
        let Some(store) = &self.store else {
            self.items.push(Item::Error("sessions are disabled".into()));
            return;
        };
        let loaded = if id == "latest" {
            store.latest().and_then(|m| match m {
                Some(m) => store.load_path(&m.path),
                None => Err(moon_core::SessionError::NotFound(
                    "no saved sessions".into(),
                )),
            })
        } else {
            store.load(id)
        };
        match loaded {
            Ok(s) => self.apply_session(s),
            Err(e) => self
                .items
                .push(Item::Error(format!("could not resume: {e}"))),
        }
    }

    // ----- startup ----------------------------------------------------------

    /// Checks providers and requests models in the background.
    pub fn bootstrap(&mut self, tx: &Tx) {
        self.loading = true;
        let registry = self.registry.clone();
        let tx_task = tx.clone();
        tokio::spawn(async move {
            let tx = tx_task;
            let futs = registry.providers().map(|(id, p)| {
                let id = id.clone();
                let p = p.clone();
                let tx = tx.clone();
                async move {
                    let h = p.health().await.map_err(|e| e.to_string());
                    let _ = tx.send(Action::ProviderChecked(id.clone(), h));
                    let m = p.list_models().await.map_err(|e| e.to_string());
                    let _ = tx.send(Action::ModelsLoaded(id, m));
                }
            });
            futures_util::future::join_all(futs).await;
            let _ = tx.send(Action::ModelsDone);
        });
        if self.current.is_some() {
            self.fetch_model_state(tx);
        }
        if self.cfg.general.system_stats {
            crate::sysmon::spawn(tx.clone(), self.sys_pace.clone());
        }
        // the loaded model is polled at the same slow pace as the machine
        let tx_poll = tx.clone();
        tokio::spawn(async move {
            let mut t = tokio::time::interval(crate::sysmon::IDLE_INTERVAL);
            loop {
                t.tick().await;
                if tx_poll.send(Action::PollLoaded).is_err() {
                    break;
                }
            }
        });
    }

    fn refresh_models(&self, tx: &Tx) {
        let registry = self.registry.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            for (id, r) in registry.list_all_models().await {
                let _ = tx.send(Action::ModelsLoaded(id, r.map_err(|e| e.to_string())));
            }
            let _ = tx.send(Action::ModelsDone);
        });
    }

    /// Asks the provider for the active model's details and whether it is loaded.
    fn fetch_model_state(&self, tx: &Tx) {
        self.fetch_model_info(tx);
        self.fetch_loaded(tx);
    }

    /// Asks the provider whether the active model is in memory. A failed
    /// query leaves `Unknown` (it will be asked again); a provider that
    /// cannot tell becomes `Unsupported` (it will not).
    fn fetch_loaded(&self, tx: &Tx) {
        let Some(cur) = self.current.clone() else {
            return;
        };
        let Some(p) = self.registry.get(&cur.provider) else {
            return;
        };
        let tx = tx.clone();
        tokio::spawn(async move {
            let state = match p.loaded().await {
                Ok(Some(list)) => match list.into_iter().find(|m| m.id == cur.model) {
                    Some(m) => LoadedState::Loaded(m),
                    None => LoadedState::NotLoaded,
                },
                Ok(None) => LoadedState::Unsupported,
                Err(_) => LoadedState::Unknown,
            };
            let _ = tx.send(Action::Loaded(cur, state));
        });
    }

    fn fetch_model_info(&self, tx: &Tx) {
        let Some(cur) = &self.current else { return };
        let Some(p) = self.registry.get(&cur.provider) else {
            return;
        };
        let model = cur.model.clone();
        let tx = tx.clone();
        tokio::spawn(async move {
            if let Ok(info) = p.model_info(&model).await {
                let _ = tx.send(Action::ModelInfo(info));
            }
        });
    }

    // ----- update -----------------------------------------------------------

    pub fn needs_tick(&self) -> bool {
        self.is_streaming() || self.loading || self.notice.is_some()
    }

    pub fn is_streaming(&self) -> bool {
        matches!(self.gen, Generation::Streaming { .. })
    }

    pub fn notify(&mut self, text: impl Into<String>) {
        self.notice = Some((text.into(), Instant::now()));
    }

    pub fn update(&mut self, action: Action, tx: &Tx) {
        let was_streaming = self.is_streaming();
        self.apply(action, tx);
        let streaming = self.is_streaming();
        // the machine is watched more closely while the model thinks or replies
        self.sys_pace.set_fast(streaming);
        // once a reply finishes the model is certainly in memory
        if was_streaming && !streaming {
            self.fetch_loaded(tx);
        }
    }

    fn apply(&mut self, action: Action, tx: &Tx) {
        match action {
            Action::Key(k) => self.handle_key(k, tx),
            Action::Paste(s) => {
                if let Some(p) = self.panel.as_mut().and_then(Panel::picker_mut) {
                    for c in s.chars().filter(|c| !c.is_control()) {
                        p.push(c);
                    }
                } else {
                    self.input.insert_str(&s);
                }
            }
            Action::Resize => {}
            Action::Tick => {
                self.spinner = self.spinner.wrapping_add(1);
                if let Some((_, at)) = &self.notice {
                    if at.elapsed() > NOTICE_TTL {
                        self.notice = None;
                    }
                }
            }
            Action::SysSample(s) => self.sys.push(s),
            // during generation the figure does not change: wait for the end;
            // a provider that cannot tell is not asked again
            Action::PollLoaded => {
                if !self.is_streaming() && self.loaded != LoadedState::Unsupported {
                    self.fetch_loaded(tx);
                }
            }
            Action::Loaded(cur, state) => {
                if self.current.as_ref() == Some(&cur) {
                    self.loaded = state;
                }
            }
            Action::ScrollBy(d) => match self.panel.as_mut() {
                Some(Panel::Help(h)) => h.scroll_by(d),
                Some(Panel::SessionAction { .. }) => {}
                // in the lists the wheel moves the cursor one at a time
                Some(panel) => {
                    if let Some(p) = panel.picker_mut() {
                        p.move_by(d.signum());
                    }
                }
                None => self.scroll_by(d),
            },
            Action::MouseDown(x, y) => self.mouse_down(x, y),
            Action::MouseDrag(x, y) => self.mouse_drag(x, y),
            Action::MouseUp(x, y) => self.mouse_up(x, y),
            Action::ProviderChecked(id, h) => {
                if let Some(p) = self.providers.iter_mut().find(|p| p.id == id) {
                    p.health = Some(h);
                }
            }
            Action::ModelsLoaded(id, res) => self.on_models_loaded(id, res, tx),
            Action::ModelsDone => {
                self.loading = false;
                if self.current.is_none() && self.models.is_empty() {
                    let all_down = self
                        .providers
                        .iter()
                        .all(|p| matches!(p.health, Some(Err(_))));
                    if all_down && !self.providers.is_empty() {
                        self.notify("no provider is responding: is Ollama running?");
                    }
                }
            }
            Action::ModelInfo(info) => {
                if self
                    .current
                    .as_ref()
                    .is_some_and(|c| c.provider == info.provider && c.model == info.id)
                {
                    self.ctx_len = info.context_length;
                    if let Some(m) = self
                        .models
                        .iter_mut()
                        .find(|m| m.provider == info.provider && m.id == info.id)
                    {
                        m.context_length = info.context_length;
                        m.caps = info.caps;
                    }
                }
            }
            Action::Stream(id, ev) => {
                if id == self.gen_id {
                    self.on_stream(ev);
                }
            }
            Action::SessionLoaded(s) => {
                self.apply_session(*s);
                self.notify("session resumed");
            }
            Action::Notice(s) => self.notify(s),
            Action::Quit => self.should_quit = true,
        }
    }

    fn on_models_loaded(&mut self, id: String, res: Result<Vec<ModelInfo>, String>, tx: &Tx) {
        match res {
            Ok(list) => {
                self.models.retain(|m| m.provider != id);
                self.models.extend(list);
                self.models.sort_by_key(|a| a.qualified());
                if let Some(p) = self.providers.iter_mut().find(|p| p.id == id) {
                    if p.health.is_none() {
                        p.health = Some(Ok(Health::default()));
                    }
                }
                match &self.current {
                    None => {
                        let usable = self.default_provider.as_deref().is_none_or(|d| d == id);
                        if usable {
                            if let Some(m) = self.models.iter().find(|m| m.provider == id) {
                                self.current = Some(Current {
                                    provider: m.provider.clone(),
                                    model: m.id.clone(),
                                });
                                self.ctx_len = m.context_length;
                                self.fetch_model_state(tx);
                            }
                        }
                    }
                    Some(c) if c.provider == id => {
                        if !self
                            .models
                            .iter()
                            .any(|m| m.provider == id && m.id == c.model)
                        {
                            self.notify(format!(
                                "model {} is not available on {}: /model to pick another",
                                c.model, id
                            ));
                        }
                    }
                    _ => {}
                }
            }
            Err(e) => {
                if let Some(p) = self.providers.iter_mut().find(|p| p.id == id) {
                    p.health = Some(Err(e));
                }
            }
        }
        if matches!(self.panel, Some(Panel::Models(_))) {
            let fresh = self.model_picker("");
            if let Some(Panel::Models(p)) = self.panel.as_mut() {
                p.update_from(fresh);
            }
        }
    }
}
