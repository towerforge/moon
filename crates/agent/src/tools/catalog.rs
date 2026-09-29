//! What the model may be allowed to do, in one catalogue: moon's own file
//! tools and a fixed list of commands, in groups, each with a permission
//! the agent's file sets: off, ask or allow. Nothing outside the
//! catalogue can be named, and no shell is ever involved: a command runs
//! as a program with its arguments as a list, from a folder inside the
//! project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use moon_core::config::ids::{CREATE_FILES, EDIT_FILES, READ_FILES, SUBFOLDERS, SUBFOLDERS_OLD};
use moon_core::Permission;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Category {
    /// moon's own: the file the way `@path` gives it, and changes shown as
    /// a diff.
    Editor,
    /// Programs that work on files, and where a command may run.
    Files,
    Git,
    /// Each language's tools, in sections: `make`, then Rust, Node and
    /// Python.
    Stack,
    Docker,
    Network,
}

impl Category {
    pub const ALL: [Category; 6] = [
        Category::Editor,
        Category::Files,
        Category::Git,
        Category::Stack,
        Category::Docker,
        Category::Network,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Editor => "Editor",
            Category::Files => "Files",
            Category::Git => "Git",
            Category::Stack => "Stack",
            Category::Docker => "Docker",
            Category::Network => "Network",
        }
    }

    /// One line on what the group is: the panel's hint under its title, and
    /// the comment over it in an agent file.
    pub fn about(self) -> &'static str {
        match self {
            Category::Editor => {
                "moon's own tools, and how the model's calls run: files read and changed with a diff, commands in subfolders, the step limit"
            }
            Category::Files => "programs that work on files, run as they are",
            Category::Git => {
                "the repository: what only looks allows by default, what changes it asks"
            }
            Category::Stack => {
                "each language's tools: they run the project's own code, so most ask by default"
            }
            Category::Docker => {
                "the project's Compose: what only looks allows, what starts, stops or runs inside asks"
            }
            Category::Network => {
                "what a command fetches goes to the model; flags that would send a file are refused"
            }
        }
    }
}

/// Inside `Stack`, the language an entry belongs to: the panel, an agent file
/// and the docs set each one apart under its title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Make,
    Rust,
    Node,
    Python,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Make => "Make",
            Section::Rust => "Rust",
            Section::Node => "Node",
            Section::Python => "Python",
        }
    }
}

/// What an entry of the catalogue is behind its name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `read_file` and `list_dir`.
    Read,
    /// `edit_file`.
    Edit,
    /// `write_file`.
    Create,
    /// Not a program: a call may carry a `dir`, a folder below the project
    /// root to run in, never one above it. Off or allow, nothing to ask.
    Subfolders,
    /// A program, the first word of the id, with the rest as its first
    /// arguments.
    Command,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    /// How it is shown, written in an agent file and, for a command, called:
    /// `read files`, `git diff`, `ls`.
    pub id: &'static str,
    pub category: Category,
    /// Its section inside `Stack`; the other groups are one list.
    pub section: Option<Section>,
    pub kind: Kind,
    /// What `enter` turns it to: `allow` for what only looks, `ask` for what
    /// changes the repository, writes files, runs the project's own code or
    /// reaches the network.
    pub on: Permission,
    /// Flags refused however they come, because they run something else,
    /// write somewhere or send a file's contents away.
    pub deny: &'static [&'static str],
    /// One of the coreutils: on Windows it comes with Git, not with the
    /// system, whose `find` is a different program.
    pub coreutils: bool,
    /// Flags of which one must be there, or the call is refused: `-d` for
    /// `docker compose up`, which would otherwise hold until the timeout.
    pub needs: &'static [&'static str],
    pub help: &'static str,
}

const GIT_DENY: &[&str] = &[
    "--exec-path",
    "--output",
    "--upload-pack",
    "--receive-pack",
    "--exec",
    "--config-env",
    "--git-dir",
    "--work-tree",
];

/// What sends a file, or its contents, out: `curl -d @.env`, `-T file`.
const CURL_DENY: &[&str] = &[
    "-d",
    "--data",
    "--data-ascii",
    "--data-binary",
    "--data-raw",
    "--data-urlencode",
    "--json",
    "-F",
    "--form",
    "--form-string",
    "-T",
    "--upload-file",
    "-K",
    "--config",
];

const WGET_DENY: &[&str] = &[
    "--post-file",
    "--body-file",
    "-i",
    "--input-file",
    "--config",
    "-e",
    "--execute",
];

const fn entry(
    id: &'static str,
    category: Category,
    kind: Kind,
    on: Permission,
    deny: &'static [&'static str],
    coreutils: bool,
    help: &'static str,
) -> Entry {
    Entry {
        id,
        category,
        section: None,
        kind,
        on,
        deny,
        coreutils,
        needs: &[],
        help,
    }
}

/// The containers go through the project's compose, and never to another
/// daemon or with another configuration.
const DOCKER_DENY: &[&str] = &["-H", "--host", "--context", "--config"];

/// The same, and `logs` does not follow: it would never end.
const DOCKER_LOGS_DENY: &[&str] = &["-H", "--host", "--context", "--config", "-f", "--follow"];

const fn docker(id: &'static str, on: Permission, help: &'static str) -> Entry {
    entry(
        id,
        Category::Docker,
        Kind::Command,
        on,
        DOCKER_DENY,
        false,
        help,
    )
}

impl Entry {
    /// The same entry, refused unless one of `flags` comes with the call.
    const fn needing(mut self, flags: &'static [&'static str]) -> Entry {
        self.needs = flags;
        self
    }
}

const fn editor(id: &'static str, kind: Kind, on: Permission, help: &'static str) -> Entry {
    entry(id, Category::Editor, kind, on, &[], false, help)
}

const fn coreutil(
    id: &'static str,
    on: Permission,
    deny: &'static [&'static str],
    help: &'static str,
) -> Entry {
    entry(id, Category::Files, Kind::Command, on, deny, true, help)
}

const fn git(id: &'static str, on: Permission, help: &'static str) -> Entry {
    entry(id, Category::Git, Kind::Command, on, GIT_DENY, false, help)
}

const fn stack(
    id: &'static str,
    section: Section,
    on: Permission,
    deny: &'static [&'static str],
    help: &'static str,
) -> Entry {
    let mut e = entry(id, Category::Stack, Kind::Command, on, deny, false, help);
    e.section = Some(section);
    e
}

const fn network(id: &'static str, deny: &'static [&'static str], help: &'static str) -> Entry {
    entry(
        id,
        Category::Network,
        Kind::Command,
        Permission::Ask,
        deny,
        false,
        help,
    )
}

pub const CATALOG: &[Entry] = &[
    editor(
        READ_FILES,
        Kind::Read,
        Permission::Allow,
        "open and list files under this directory",
    ),
    editor(
        EDIT_FILES,
        Kind::Edit,
        Permission::Ask,
        "a diff you apply or skip; allow writes it without showing you",
    ),
    editor(
        CREATE_FILES,
        Kind::Create,
        Permission::Ask,
        "a new file you apply or skip; allow writes it without showing you",
    ),
    entry(
        SUBFOLDERS,
        Category::Editor,
        Kind::Subfolders,
        Permission::Allow,
        &[],
        false,
        "in a folder below this one, never above it",
    ),
    coreutil("ls", Permission::Allow, &[], "list a folder"),
    coreutil("cat", Permission::Allow, &[], "print a file"),
    coreutil("head", Permission::Allow, &[], "the first lines of a file"),
    coreutil("tail", Permission::Allow, &[], "the last lines of a file"),
    coreutil("wc", Permission::Allow, &[], "count lines, words and bytes"),
    coreutil("grep", Permission::Allow, &[], "search text in files"),
    coreutil(
        "find",
        Permission::Allow,
        &[
            "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf",
            "-fls",
        ],
        "find files by name",
    ),
    coreutil("tree", Permission::Allow, &["-o"], "the folder tree"),
    coreutil(
        "pwd",
        Permission::Allow,
        &[],
        "the folder the command runs in",
    ),
    coreutil("mkdir", Permission::Ask, &[], "create a folder"),
    git(
        "git status",
        Permission::Allow,
        "what is changed, staged and untracked",
    ),
    git(
        "git diff",
        Permission::Allow,
        "the changes not yet committed",
    ),
    git("git log", Permission::Allow, "the history"),
    git(
        "git show",
        Permission::Allow,
        "one commit, or a file as it was in one",
    ),
    git(
        "git blame",
        Permission::Allow,
        "who changed each line of a file",
    ),
    git("git add", Permission::Ask, "stage changes"),
    git("git commit", Permission::Ask, "commit what is staged"),
    stack(
        "make",
        Section::Make,
        Permission::Ask,
        &["--eval", "-E"],
        "run a Makefile target",
    ),
    stack(
        "cargo check",
        Section::Rust,
        Permission::Allow,
        &["--config", "-Z"],
        "compile without building",
    ),
    stack(
        "cargo clippy",
        Section::Rust,
        Permission::Allow,
        &["--config", "-Z"],
        "the lints",
    ),
    stack(
        "cargo build",
        Section::Rust,
        Permission::Allow,
        &["--config", "-Z"],
        "build",
    ),
    stack(
        "cargo test",
        Section::Rust,
        Permission::Ask,
        &["--config", "-Z"],
        "run the tests",
    ),
    stack(
        "cargo fmt",
        Section::Rust,
        Permission::Ask,
        &["--config", "-Z"],
        "format the code in place",
    ),
    stack(
        "npm run",
        Section::Node,
        Permission::Ask,
        &[],
        "run a package.json script",
    ),
    stack(
        "npm test",
        Section::Node,
        Permission::Ask,
        &[],
        "run the tests",
    ),
    stack(
        "npm install",
        Section::Node,
        Permission::Ask,
        &[],
        "install dependencies; reaches the network and runs their scripts",
    ),
    stack(
        "npm ci",
        Section::Node,
        Permission::Ask,
        &[],
        "a clean install from the lockfile",
    ),
    stack(
        "npm ls",
        Section::Node,
        Permission::Allow,
        &[],
        "the dependency tree",
    ),
    stack(
        "pnpm run",
        Section::Node,
        Permission::Ask,
        &[],
        "run a package.json script",
    ),
    stack(
        "pnpm test",
        Section::Node,
        Permission::Ask,
        &[],
        "run the tests",
    ),
    stack(
        "pnpm install",
        Section::Node,
        Permission::Ask,
        &[],
        "install dependencies; reaches the network and runs their scripts",
    ),
    stack(
        "pnpm list",
        Section::Node,
        Permission::Allow,
        &[],
        "the dependency tree",
    ),
    stack(
        "pytest",
        Section::Python,
        Permission::Ask,
        &[],
        "run the tests",
    ),
    stack(
        "python3",
        Section::Python,
        Permission::Ask,
        &[],
        "run a Python script of the project",
    ),
    stack(
        "pip install",
        Section::Python,
        Permission::Ask,
        &[],
        "install packages; reaches the network",
    ),
    stack(
        "pip list",
        Section::Python,
        Permission::Allow,
        &[],
        "the installed packages",
    ),
    docker("docker ps", Permission::Allow, "the running containers"),
    docker(
        "docker compose ps",
        Permission::Allow,
        "the services of the compose",
    ),
    entry(
        "docker compose logs",
        Category::Docker,
        Kind::Command,
        Permission::Allow,
        DOCKER_LOGS_DENY,
        false,
        "a service's logs, without following them",
    ),
    docker(
        "docker compose config",
        Permission::Allow,
        "the compose as it resolves",
    ),
    docker("docker compose build", Permission::Ask, "build the images"),
    docker(
        "docker compose up",
        Permission::Ask,
        "start the services, detached: `-d` is required",
    )
    .needing(&["-d", "--detach"]),
    docker("docker compose down", Permission::Ask, "stop the services"),
    docker(
        "docker compose restart",
        Permission::Ask,
        "restart a service",
    ),
    docker(
        "docker compose exec",
        Permission::Ask,
        "run a command inside a service",
    ),
    network(
        "curl",
        CURL_DENY,
        "fetch a URL; what it gets goes to the model",
    ),
    network("wget", WGET_DENY, "download a URL into the project"),
];

impl Entry {
    pub fn is_command(&self) -> bool {
        self.kind == Kind::Command
    }

    pub fn words(&self) -> impl Iterator<Item = &'static str> {
        self.id.split(' ')
    }

    /// The program of a command: the first word of the id.
    pub fn program(&self) -> &'static str {
        self.id.split(' ').next().unwrap_or("")
    }

    /// The arguments every call starts with: `["diff"]` for `git diff`.
    pub fn prefix(&self) -> Vec<String> {
        self.id.split(' ').skip(1).map(String::from).collect()
    }

    /// Where the program is on this machine, if it is; what is not a
    /// program is always there.
    pub fn resolve(&self) -> Option<PathBuf> {
        if !self.is_command() {
            return Some(PathBuf::new());
        }
        resolve(self.program(), self.coreutils)
    }
}

const TABLE_HEAD: &str =
    "| Name | Default when turned on | What it does | Refused flags |\n|---|---|---|---|\n";

/// The catalogue as a Markdown table, one per group or section, for the docs: what is
/// listed there is what the code has, and a test says when they part.
pub fn markdown_table() -> String {
    let mut out = String::new();
    for cat in Category::ALL {
        out.push_str(&format!("**{}** — {}\n\n", cat.title(), cat.about()));
        let mut section = None;
        for (n, e) in CATALOG.iter().filter(|e| e.category == cat).enumerate() {
            // a table for the group, or one for each of its sections
            if n == 0 || e.section != section {
                if n > 0 {
                    out.push('\n');
                }
                if let Some(s) = e.section {
                    out.push_str(&format!("*{}*\n\n", s.title()));
                }
                section = e.section;
                out.push_str(TABLE_HEAD);
            }
            let deny = if e.deny.is_empty() {
                "—".to_string()
            } else {
                e.deny
                    .iter()
                    .map(|d| format!("`{d}`"))
                    .collect::<Vec<_>>()
                    .join(" ")
            };
            out.push_str(&format!(
                "| `{}` | `{}` | {} | {} |\n",
                e.id,
                e.on.as_str(),
                e.help,
                deny
            ));
        }
        out.push('\n');
    }
    out
}

pub fn find(id: &str) -> Option<&'static Entry> {
    let id = if id == SUBFOLDERS_OLD { SUBFOLDERS } else { id };
    CATALOG.iter().find(|e| e.id == id)
}

/// The permission of every entry of the catalogue; what is not in the map
/// is off. The one rule it keeps on its own: editing and creating files
/// need reading, so turning them on turns `read files` on, and turning
/// `read files` off turns them off.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Policy {
    map: BTreeMap<&'static str, Permission>,
}

impl Policy {
    /// From the pairs a file or a configuration gives: an id the catalogue
    /// does not have is dropped, and the rule between reading and writing
    /// is applied.
    pub fn from_pairs<K: AsRef<str>>(pairs: impl IntoIterator<Item = (K, Permission)>) -> Self {
        let mut p = Policy::default();
        for (id, perm) in pairs {
            p.set(id.as_ref(), perm);
        }
        p
    }

    pub fn get(&self, id: &str) -> Permission {
        self.map.get(id).copied().unwrap_or(Permission::Off)
    }

    /// Not off.
    pub fn allows(&self, id: &str) -> bool {
        self.get(id) != Permission::Off
    }

    pub fn set(&mut self, id: &str, perm: Permission) {
        let Some(entry) = find(id) else {
            return;
        };
        // where a command may run is yes or no: there is nothing to ask
        let perm = match (entry.kind, perm) {
            (Kind::Subfolders, Permission::Ask) => Permission::Allow,
            (_, p) => p,
        };
        if perm == Permission::Off {
            self.map.remove(entry.id);
        } else {
            self.map.insert(entry.id, perm);
        }
        match entry.kind {
            Kind::Edit | Kind::Create if perm != Permission::Off => {
                if !self.allows(READ_FILES) {
                    self.map.insert(READ_FILES, Permission::Allow);
                }
            }
            Kind::Read if perm == Permission::Off => {
                self.map.remove(EDIT_FILES);
                self.map.remove(CREATE_FILES);
            }
            _ => {}
        }
    }

    /// Nothing is on: tools off.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn reads(&self) -> bool {
        self.allows(READ_FILES)
    }

    pub fn edits(&self) -> bool {
        self.allows(EDIT_FILES)
    }

    pub fn creates(&self) -> bool {
        self.allows(CREATE_FILES)
    }

    pub fn subfolders(&self) -> bool {
        self.allows(SUBFOLDERS)
    }

    /// A write lands without being shown first: editing or creating at
    /// `allow`.
    pub fn writes_unseen(&self) -> bool {
        self.get(EDIT_FILES) == Permission::Allow || self.get(CREATE_FILES) == Permission::Allow
    }

    /// The commands that are on, in catalogue order.
    pub fn commands(&self) -> Vec<&'static Entry> {
        CATALOG
            .iter()
            .filter(|e| e.is_command() && self.allows(e.id))
            .collect()
    }

    /// Everything that is on, with its permission, in catalogue order.
    pub fn entries(&self) -> Vec<(&'static Entry, Permission)> {
        CATALOG
            .iter()
            .filter_map(|e| {
                let p = self.get(e.id);
                (p != Permission::Off).then_some((e, p))
            })
            .collect()
    }

    /// For the file: id and permission of everything that is on.
    pub fn pairs(&self) -> BTreeMap<String, Permission> {
        self.map.iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    /// How many are on, and how many of those ask.
    pub fn counts(&self) -> (usize, usize) {
        let asks = self.map.values().filter(|p| **p == Permission::Ask).count();
        (self.map.len(), asks)
    }
}

/// The command a call names: the longest id whose words open the tokens,
/// among the ones that are on. The tokens left are the arguments.
pub fn match_call(policy: &Policy, tokens: &[String]) -> Option<(&'static Entry, usize)> {
    policy
        .commands()
        .into_iter()
        .filter_map(|e| {
            let n = e.words().count();
            let opens = tokens.len() >= n && e.words().zip(tokens).all(|(w, t)| w == t);
            opens.then_some((e, n))
        })
        .max_by_key(|(_, n)| *n)
}

/// Why an argument is refused: a flag on the deny list, or a path that
/// leaves the project. Anything that could be a path is checked as one,
/// value of a `--flag=value` included, so `cat /etc/passwd` and
/// `git diff ../x` never run.
pub fn check_arg(entry: &Entry, arg: &str) -> Result<(), String> {
    if arg.chars().any(char::is_control) {
        return Err("arguments cannot contain control characters".into());
    }
    let flag = arg.split_once('=').map_or(arg, |(f, _)| f);
    if arg.starts_with('-') && entry.deny.contains(&flag) {
        return Err(format!("`{flag}` is not allowed with {}", entry.id));
    }
    let value = if arg.starts_with('-') {
        arg.split_once('=').map_or("", |(_, v)| v)
    } else {
        arg
    };
    check_path_like(value)
}

fn check_path_like(v: &str) -> Result<(), String> {
    let norm = v.replace('\\', "/");
    let mut chars = norm.chars();
    let first = chars.next();
    let second = chars.next();
    let third = chars.next();
    let drive = first.is_some_and(|c| c.is_ascii_alphabetic())
        && second == Some(':')
        && third.is_none_or(|c| c == '/');
    if first == Some('/') || first == Some('~') || drive {
        return Err(format!(
            "`{v}` is outside the project: paths are relative to the project root, without `~`"
        ));
    }
    if norm.split('/').any(|c| c == "..") {
        return Err(format!("`{v}` is outside the project"));
    }
    Ok(())
}

/// The executable on `PATH`, with the extensions Windows appends. The
/// coreutils on Windows are looked for next to Git only: the system's own
/// `find` is another program.
pub fn resolve(program: &str, coreutils: bool) -> Option<PathBuf> {
    if program.is_empty() {
        return None;
    }
    if cfg!(windows) && coreutils {
        return git_usr_bin().and_then(|d| candidate(&d, program));
    }
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .filter(|d| !d.as_os_str().is_empty())
        .find_map(|d| candidate(&d, program))
}

/// `usr/bin` of Git for Windows, where its `ls`, `cat` and `grep` live,
/// found from wherever `git.exe` is on `PATH`.
fn git_usr_bin() -> Option<PathBuf> {
    let git = resolve("git", false)?;
    git.ancestors()
        .skip(1)
        .take(4)
        .map(|a| a.join("usr").join("bin"))
        .find(|d| d.is_dir())
}

fn candidate(dir: &Path, program: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .map(|p| {
                p.split(';')
                    .filter(|e| !e.is_empty())
                    .map(|e| e.to_ascii_lowercase())
                    .collect()
            })
            .unwrap_or_else(|_| {
                [".com", ".exe", ".bat", ".cmd"]
                    .iter()
                    .map(|e| e.to_string())
                    .collect()
            })
    } else {
        vec![String::new()]
    };
    exts.iter()
        .map(|e| dir.join(format!("{program}{e}")))
        .find(|p| is_executable(p))
}

fn is_executable(p: &Path) -> bool {
    let Ok(m) = std::fs::metadata(p) else {
        return false;
    };
    if !m.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests;
