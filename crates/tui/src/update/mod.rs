//! `moon update`: what GitHub published, and the box that installs it.
//!
//! The logic is `moon-updater`'s; here is the terminal. With a terminal it is
//! an inline panel that asks before touching anything; piped, it is a handful
//! of lines and `--yes` is required to install.

mod view;

use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crossterm::event::{Event, EventStream, KeyCode, KeyEventKind, KeyModifiers};
use futures_util::StreamExt;
use moon_updater::{
    cache::CheckCache, install, unpack, verify_sha256, Client, Release, Target, UpdateError,
    Version,
};
use ratatui::{TerminalOptions, Viewport};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::app::TICK;
use crate::theme::Theme;

/// Where the installer is written down, for when the update cannot go on. It
/// only installs over an existing moon with `MOON_FORCE=1`: the rest of the
/// time it sends you back here.
pub const INSTALL_DOCS: &str = "github.com/towerforge/moon#installation";

pub struct Options {
    /// Version that is running.
    pub current: Version,
    /// A specific version instead of the latest one.
    pub to: Option<Version>,
    /// Look, say what there is, install nothing.
    pub check_only: bool,
    /// Install without asking.
    pub yes: bool,
    /// Install over a cargo install or a build under `target/`, and reinstall
    /// a version that is already there.
    pub force: bool,
    /// Where the daily check writes what it found. `None` remembers nothing.
    pub state_dir: Option<PathBuf>,
    pub theme: Theme,
}

impl Options {
    /// Whether that release is worth installing: a newer one always is, the
    /// one that was asked for by name is too — a downgrade is a decision, not
    /// a mistake — and `--force` settles the rest.
    fn worth_installing(&self, release: &Version) -> bool {
        *release > self.current || self.force || (self.to.is_some() && *release != self.current)
    }
}

/// How it ended, for the exit status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    UpToDate,
    /// `--check` and there is something newer.
    Available(Version),
    Installed(Version),
    Cancelled,
    /// Already shown in the box or on stderr: the caller only sets the status.
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Download { got: u64, total: Option<u64> },
    Verify,
    Install,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Checking,
    UpToDate,
    /// `--check`: there is a new version and nothing more will happen.
    Available,
    /// Waiting for the go-ahead.
    Confirm,
    Working(Step),
    Done,
    Cancelled,
    Failed(String),
}

pub struct State {
    pub current: Version,
    pub target: Target,
    pub dest: PathBuf,
    pub release: Option<Release>,
    pub phase: Phase,
    pub spinner: usize,
    /// Whether the archive could be checked against `checksums.txt`.
    pub verified: bool,
}

impl State {
    pub fn new(current: Version, target: Target, dest: PathBuf) -> Self {
        Self {
            current,
            target,
            dest,
            release: None,
            phase: Phase::Checking,
            spinner: 0,
            verified: true,
        }
    }

    /// The version that would be installed, once it is known that it is worth
    /// installing.
    pub fn next_version(&self) -> Option<&Version> {
        match self.phase {
            Phase::UpToDate | Phase::Checking => None,
            _ => self.release.as_ref().map(|r| &r.version),
        }
    }

    pub fn asset_name(&self) -> String {
        self.target.asset_name()
    }

    pub fn asset_size(&self) -> Option<u64> {
        self.release
            .as_ref()
            .and_then(|r| r.asset_for(&self.target).ok())
            .map(|a| a.size)
    }

    /// The path with the home directory as `~`, which is where it usually is.
    pub fn dest_display(&self) -> String {
        let s = self.dest.display().to_string();
        match std::env::var("HOME") {
            Ok(h) if !h.is_empty() && s.starts_with(&h) => format!("~{}", &s[h.len()..]),
            _ => s,
        }
    }

    /// Whether the panel has something moving in it.
    fn animating(&self) -> bool {
        matches!(self.phase, Phase::Checking | Phase::Working(_))
    }

    fn outcome(&self) -> Outcome {
        match &self.phase {
            Phase::UpToDate => Outcome::UpToDate,
            Phase::Available => self
                .release
                .as_ref()
                .map(|r| Outcome::Available(r.version.clone()))
                .unwrap_or(Outcome::UpToDate),
            Phase::Done => self
                .release
                .as_ref()
                .map(|r| Outcome::Installed(r.version.clone()))
                .unwrap_or(Outcome::UpToDate),
            Phase::Failed(_) => Outcome::Failed,
            _ => Outcome::Cancelled,
        }
    }
}

/// What the background work tells the panel.
enum Msg {
    Release(Result<Release, String>),
    Progress(u64, Option<u64>),
    Step(Step),
    /// Installed, and whether the checksum could be verified.
    Finished(Result<bool, String>),
}

pub async fn run(opts: Options) -> anyhow::Result<Outcome> {
    let target = Target::current()?;
    let dest = install::current_exe()?;
    let state = State::new(opts.current.clone(), target, dest);
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return piped(opts, state).await;
    }
    panel(opts, state).await
}

// ----- the panel ------------------------------------------------------------

async fn panel(opts: Options, mut state: State) -> anyhow::Result<Outcome> {
    // the inline viewport needs to know where the cursor is; a terminal that
    // will not say is no reason to refuse to update
    let inline = TerminalOptions {
        viewport: Viewport::Inline(view::VIEWPORT),
    };
    let Ok(mut term) = ratatui::try_init_with_options(inline) else {
        ratatui::restore();
        return piped(opts, state).await;
    };
    let result = drive(&mut term, &mut state, &opts).await;
    // the last frame stays above the prompt; the live viewport goes
    let _ = term.insert_before(view::VIEWPORT, |buf| {
        view::render(&state, &opts.theme, buf.area, buf);
    });
    let _ = term.clear();
    ratatui::restore();
    result
}

async fn drive(
    term: &mut ratatui::DefaultTerminal,
    state: &mut State,
    opts: &Options,
) -> anyhow::Result<Outcome> {
    let client = Arc::new(Client::new()?);
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    let mut events = EventStream::new();
    let mut tick = tokio::time::interval(TICK);
    let mut worker: Option<JoinHandle<()>> = Some(spawn_check(&client, opts.to.clone(), &tx));

    loop {
        term.draw(|f| view::render(state, &opts.theme, f.area(), f.buffer_mut()))?;
        tokio::select! {
            ev = events.next() => match ev {
                Some(Ok(Event::Key(k))) if k.kind != KeyEventKind::Release => {
                    let ctrl_c = k.code == KeyCode::Char('c')
                        && k.modifiers.contains(KeyModifiers::CONTROL);
                    match k.code {
                        KeyCode::Enter | KeyCode::Char('y') if state.phase == Phase::Confirm => {
                            worker = Some(spawn_install(&client, state, &tx));
                            state.phase = Phase::Working(Step::Download { got: 0, total: None });
                        }
                        _ if ctrl_c || matches!(k.code, KeyCode::Esc | KeyCode::Char('q')) => {
                            if let Some(w) = worker.take() {
                                w.abort();
                            }
                            state.phase = Phase::Cancelled;
                            return Ok(Outcome::Cancelled);
                        }
                        _ => {}
                    }
                }
                Some(Ok(_)) => {}
                Some(Err(_)) | None => return Ok(state.outcome()),
            },
            Some(msg) = rx.recv() => {
                if let Some(done) = apply(msg, state, opts, &client, &tx, &mut worker) {
                    return Ok(done);
                }
            }
            _ = tick.tick(), if state.animating() => state.spinner = state.spinner.wrapping_add(1),
        }
    }
}

/// Applies what the background work says. `Some(outcome)` ends the panel.
fn apply(
    msg: Msg,
    state: &mut State,
    opts: &Options,
    client: &Arc<Client>,
    tx: &mpsc::UnboundedSender<Msg>,
    worker: &mut Option<JoinHandle<()>>,
) -> Option<Outcome> {
    match msg {
        Msg::Release(Ok(release)) => {
            remember(opts.state_dir.as_deref(), &release.version);
            let worth_it = opts.worth_installing(&release.version);
            state.release = Some(release);
            if !worth_it {
                state.phase = Phase::UpToDate;
                return Some(Outcome::UpToDate);
            }
            if opts.check_only {
                state.phase = Phase::Available;
                return Some(state.outcome());
            }
            if let Err(why) = allowed(&state.dest, opts.force) {
                state.phase = Phase::Failed(why);
                return Some(Outcome::Failed);
            }
            if opts.yes {
                *worker = Some(spawn_install(client, state, tx));
                state.phase = Phase::Working(Step::Download {
                    got: 0,
                    total: state.asset_size(),
                });
            } else {
                state.phase = Phase::Confirm;
            }
            None
        }
        Msg::Release(Err(e)) => {
            state.phase = Phase::Failed(e);
            Some(Outcome::Failed)
        }
        Msg::Progress(got, total) => {
            state.phase = Phase::Working(Step::Download { got, total });
            None
        }
        Msg::Step(s) => {
            state.phase = Phase::Working(s);
            None
        }
        Msg::Finished(Ok(verified)) => {
            state.verified = verified;
            state.phase = Phase::Done;
            // the notice must not outlive the update that answered it
            if let Some(dir) = opts.state_dir.as_deref() {
                CheckCache::forget(dir);
            }
            Some(state.outcome())
        }
        Msg::Finished(Err(e)) => {
            state.phase = Phase::Failed(e);
            Some(Outcome::Failed)
        }
    }
}

fn spawn_check(
    client: &Arc<Client>,
    to: Option<Version>,
    tx: &mpsc::UnboundedSender<Msg>,
) -> JoinHandle<()> {
    let (client, tx) = (client.clone(), tx.clone());
    tokio::spawn(async move {
        let res = fetch(&client, to.as_ref()).await.map_err(|e| e.to_string());
        let _ = tx.send(Msg::Release(res));
    })
}

fn spawn_install(
    client: &Arc<Client>,
    state: &State,
    tx: &mpsc::UnboundedSender<Msg>,
) -> JoinHandle<()> {
    let (client, tx) = (client.clone(), tx.clone());
    let release = state.release.clone();
    let (target, dest) = (state.target, state.dest.clone());
    tokio::spawn(async move {
        let Some(release) = release else { return };
        let res = install_release(&client, &release, &target, &dest, &tx).await;
        let _ = tx.send(Msg::Finished(res.map_err(|e| e.to_string())));
    })
}

// ----- the work itself ------------------------------------------------------

async fn fetch(client: &Client, to: Option<&Version>) -> Result<Release, UpdateError> {
    match to {
        Some(v) => client.release(v).await,
        None => client.latest().await,
    }
}

/// Downloads the archive, checks it, and puts the binary in place. Returns
/// whether the checksum could be verified.
async fn install_release(
    client: &Client,
    release: &Release,
    target: &Target,
    dest: &Path,
    tx: &mpsc::UnboundedSender<Msg>,
) -> Result<bool, UpdateError> {
    let asset = release.asset_for(target)?.clone();
    let archive = client
        .download(&asset.url, |got, total| {
            let _ = tx.send(Msg::Progress(got, total));
        })
        .await?;
    let _ = tx.send(Msg::Step(Step::Verify));
    // a release with no checksums.txt is still an HTTPS download from the
    // repository: it goes on, and the panel says it could not be verified
    let verified = match release.checksums_url() {
        Some(url) => match client.text(url).await {
            Ok(sums) => verify_sha256(&archive, &sums, &asset.name)?,
            Err(_) => false,
        },
        None => false,
    };
    let _ = tx.send(Msg::Step(Step::Install));
    let binary = unpack(&archive, target)?;
    install::install_binary(&binary, dest)?;
    Ok(verified)
}

/// Whether this binary is ours to replace, and whether we can write there.
fn allowed(dest: &Path, force: bool) -> Result<(), String> {
    if !force {
        if let Some(why) = install::classify(dest).refusal() {
            return Err(why.to_string());
        }
    }
    install::check_writable(dest).map_err(|e| e.to_string())
}

fn remember(state_dir: Option<&Path>, latest: &Version) {
    if let Some(dir) = state_dir {
        let _ = CheckCache::new(latest).save(dir);
    }
}

// ----- piped ----------------------------------------------------------------

/// No terminal: plain lines, and nothing is installed without `--yes`.
async fn piped(opts: Options, mut state: State) -> anyhow::Result<Outcome> {
    let client = Client::new()?;
    let release = fetch(&client, opts.to.as_ref()).await?;
    remember(opts.state_dir.as_deref(), &release.version);
    let worth_it = opts.worth_installing(&release.version);
    let next = release.version.clone();
    state.release = Some(release);

    if !worth_it {
        println!("moon {} is the latest version", state.current);
        return Ok(Outcome::UpToDate);
    }
    if opts.check_only {
        println!("moon {next} is available · run `moon update`");
        return Ok(Outcome::Available(next));
    }
    if !opts.yes {
        anyhow::bail!("no interactive terminal: pass --yes to update without asking");
    }
    if let Err(why) = allowed(&state.dest, opts.force) {
        anyhow::bail!(why);
    }
    println!("moon {} → {next}", state.current);
    println!("downloading {}", state.asset_name());
    let (tx, mut rx) = mpsc::unbounded_channel::<Msg>();
    // nobody is watching the progress here: drain it and keep the channel open
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let release = state.release.clone().expect("fetched above");
    let verified = install_release(&client, &release, &state.target, &state.dest, &tx).await?;
    if let Some(dir) = opts.state_dir.as_deref() {
        CheckCache::forget(dir);
    }
    println!(
        "installed moon {next} to {}{}",
        state.dest.display(),
        if verified {
            ""
        } else {
            " (checksum not published: not verified)"
        }
    );
    Ok(Outcome::Installed(next))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state() -> State {
        state_at(PathBuf::from("/usr/local/bin/moon"))
    }

    fn state_at(dest: PathBuf) -> State {
        State::new(
            "0.1.1".parse().unwrap(),
            Target::new("linux", "x86_64", false),
            dest,
        )
    }

    fn release(version: &str) -> Release {
        Release {
            version: version.parse().unwrap(),
            tag: format!("v{version}"),
            notes: String::new(),
            url: String::new(),
            published_at: None,
            assets: vec![moon_updater::Asset {
                name: "moon-linux-x86_64.tar.gz".into(),
                url: String::new(),
                size: 4_000_000,
            }],
        }
    }

    fn opts(check_only: bool, yes: bool) -> Options {
        Options {
            current: "0.1.1".parse().unwrap(),
            to: None,
            check_only,
            yes,
            force: false,
            state_dir: None,
            theme: Theme::moon(),
        }
    }

    /// `apply` with the pieces the panel would hold.
    struct Harness {
        client: Arc<Client>,
        tx: mpsc::UnboundedSender<Msg>,
        _rx: mpsc::UnboundedReceiver<Msg>,
        worker: Option<JoinHandle<()>>,
    }

    impl Harness {
        fn new() -> Self {
            let (tx, _rx) = mpsc::unbounded_channel();
            Self {
                client: Arc::new(Client::new().expect("http client")),
                tx,
                _rx,
                worker: None,
            }
        }

        fn apply(&mut self, msg: Msg, state: &mut State, opts: &Options) -> Option<Outcome> {
            apply(msg, state, opts, &self.client, &self.tx, &mut self.worker)
        }
    }

    #[tokio::test]
    async fn an_old_release_is_not_news() {
        let mut h = Harness::new();
        let mut s = state();
        let done = h.apply(
            Msg::Release(Ok(release("0.1.0"))),
            &mut s,
            &opts(false, false),
        );
        assert_eq!(done, Some(Outcome::UpToDate));
        assert_eq!(s.phase, Phase::UpToDate);
        assert!(h.worker.is_none());
    }

    #[tokio::test]
    async fn a_new_release_waits_for_the_go_ahead() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = Harness::new();
        let mut s = state_at(dir.path().join("moon"));
        let done = h.apply(
            Msg::Release(Ok(release("0.2.0"))),
            &mut s,
            &opts(false, false),
        );
        assert_eq!(done, None);
        assert_eq!(s.phase, Phase::Confirm);
        // nothing is downloaded until enter is pressed
        assert!(h.worker.is_none());
    }

    #[tokio::test]
    async fn with_check_it_reports_and_touches_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let mut h = Harness::new();
        let mut s = state_at(dir.path().join("moon"));
        let done = h.apply(
            Msg::Release(Ok(release("0.2.0"))),
            &mut s,
            &opts(true, false),
        );
        assert_eq!(done, Some(Outcome::Available("0.2.0".parse().unwrap())));
        assert_eq!(s.phase, Phase::Available);
        assert!(h.worker.is_none());
    }

    #[tokio::test]
    async fn on_a_local_build_it_stops_before_downloading() {
        let mut h = Harness::new();
        let mut s = state_at(PathBuf::from("/home/j/moon/target/release/moon"));
        let done = h.apply(
            Msg::Release(Ok(release("0.2.0"))),
            &mut s,
            &opts(false, true),
        );
        assert_eq!(done, Some(Outcome::Failed));
        assert!(matches!(&s.phase, Phase::Failed(e) if e.contains("make build")));
        assert!(h.worker.is_none());
    }

    #[tokio::test]
    async fn the_end_says_whether_it_could_verify() {
        let mut h = Harness::new();
        let mut s = state();
        s.release = Some(release("0.2.0"));
        s.phase = Phase::Working(Step::Install);
        let done = h.apply(Msg::Finished(Ok(false)), &mut s, &opts(false, true));
        assert_eq!(done, Some(Outcome::Installed("0.2.0".parse().unwrap())));
        assert_eq!(s.phase, Phase::Done);
        assert!(!s.verified, "no checksums.txt: it has to be said");
    }

    #[tokio::test]
    async fn a_github_failure_is_shown_and_ends_there() {
        let mut h = Harness::new();
        let mut s = state();
        let done = h.apply(
            Msg::Release(Err("github: rate limit reached".into())),
            &mut s,
            &opts(false, false),
        );
        assert_eq!(done, Some(Outcome::Failed));
        assert!(matches!(&s.phase, Phase::Failed(e) if e.contains("rate limit")));
    }

    #[test]
    fn to_allows_a_downgrade_but_not_the_same_version_again() {
        let mut o = opts(false, false);
        o.to = Some("0.1.0".parse().unwrap());
        assert!(o.worth_installing(&"0.1.0".parse().unwrap()));
        // asking for the one that is already running is not worth a download
        o.to = Some("0.1.1".parse().unwrap());
        assert!(!o.worth_installing(&"0.1.1".parse().unwrap()));
        // …unless it is asked for outright
        o.force = true;
        assert!(o.worth_installing(&"0.1.1".parse().unwrap()));
    }

    #[test]
    fn the_version_that_would_install_shows_only_when_due() {
        let mut s = state();
        assert_eq!(s.next_version(), None);
        s.release = Some(Release {
            version: "0.2.0".parse().unwrap(),
            tag: "v0.2.0".into(),
            notes: String::new(),
            url: String::new(),
            published_at: None,
            assets: Vec::new(),
        });
        // while it is checking there is nothing to promise
        assert_eq!(s.next_version(), None);
        s.phase = Phase::Confirm;
        assert_eq!(
            s.next_version().map(|v| v.to_string()),
            Some("0.2.0".into())
        );
        s.phase = Phase::UpToDate;
        assert_eq!(s.next_version(), None);
    }

    #[test]
    fn without_force_neither_a_cargo_install_nor_a_build_is_overwritten() {
        let cargo = PathBuf::from("/home/j/.cargo/bin/moon");
        assert!(allowed(&cargo, false).is_err());
        let dev = PathBuf::from("/home/j/moon/target/release/moon");
        assert!(allowed(&dev, false).unwrap_err().contains("make build"));
        // with --force the only thing left to check is the permission
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("moon");
        assert!(allowed(&dest, true).is_ok());
    }

    #[test]
    fn a_directory_that_does_not_exist_is_not_writable() {
        let dest = PathBuf::from("/no/such/dir/moon");
        assert!(allowed(&dest, true).is_err());
    }

    #[test]
    fn home_is_shortened_in_the_path() {
        let mut s = state();
        let home = std::env::var("HOME").unwrap_or_default();
        if home.is_empty() {
            return;
        }
        s.dest = PathBuf::from(format!("{home}/.local/bin/moon"));
        assert_eq!(s.dest_display(), "~/.local/bin/moon");
    }
}
