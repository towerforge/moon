//! What the model may be allowed to do, in one catalogue: moon's own file
//! tools and a fixed list of commands, in groups, each with a permission
//! the user sets in `/tools`: off, ask or allow. Nothing outside the
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
    Build,
    Network,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Editor,
        Category::Files,
        Category::Git,
        Category::Build,
        Category::Network,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Editor => "Editor",
            Category::Files => "Files",
            Category::Git => "Git",
            Category::Build => "Build",
            Category::Network => "Network",
        }
    }

    /// One line on what the group is: the panel's hint under its title, and
    /// the comment over it in `tools.toml`.
    pub fn about(self) -> &'static str {
        match self {
            Category::Editor => {
                "moon's own tools, and how the model's calls run: files read and changed with a diff, commands in subfolders, the step limit"
            }
            Category::Files => "programs that work on files, run as they are",
            Category::Git => {
                "the repository: what only looks allows by default, what changes it asks"
            }
            Category::Build => {
                "builds and tests: they run the project's own code, so most ask by default"
            }
            Category::Network => {
                "what a command fetches goes to the model; flags that would send a file are refused"
            }
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
    /// How it is shown, written in `tools.toml` and, for a command, called:
    /// `read files`, `git diff`, `ls`.
    pub id: &'static str,
    pub category: Category,
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
        kind,
        on,
        deny,
        coreutils,
        help,
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

const fn build(
    id: &'static str,
    on: Permission,
    deny: &'static [&'static str],
    help: &'static str,
) -> Entry {
    entry(id, Category::Build, Kind::Command, on, deny, false, help)
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
    build(
        "make",
        Permission::Ask,
        &["--eval", "-E"],
        "run a Makefile target",
    ),
    build(
        "cargo check",
        Permission::Allow,
        &["--config", "-Z"],
        "compile without building",
    ),
    build(
        "cargo clippy",
        Permission::Allow,
        &["--config", "-Z"],
        "the lints",
    ),
    build(
        "cargo build",
        Permission::Allow,
        &["--config", "-Z"],
        "build",
    ),
    build(
        "cargo test",
        Permission::Ask,
        &["--config", "-Z"],
        "run the tests",
    ),
    build(
        "cargo fmt",
        Permission::Ask,
        &["--config", "-Z"],
        "format the code in place",
    ),
    build("npm run", Permission::Ask, &[], "run a package.json script"),
    build("npm test", Permission::Ask, &[], "run the tests"),
    build("pytest", Permission::Ask, &[], "run the tests"),
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

/// `tools.toml` as moon writes it: the header, the step limit, and every
/// entry of the catalogue in its group, each with its permission, `off`
/// included, and what it does as a comment, so the file shows everything
/// there is to set.
pub fn render_tools_file(policy: &Policy, max_steps: usize) -> String {
    let key_w = CATALOG.iter().map(|e| e.id.len() + 2).max().unwrap_or(10);
    let mut out = String::from(moon_core::config::TOOLS_FILE_HEADER);
    out.push_str(&format!("max_steps = {max_steps}\n\n[permissions]\n"));
    for (n, cat) in Category::ALL.into_iter().enumerate() {
        if n > 0 {
            out.push('\n');
        }
        out.push_str(&format!("# {} — {}\n", cat.title(), cat.about()));
        for e in CATALOG.iter().filter(|e| e.category == cat) {
            let key = format!("\"{}\"", e.id);
            let value = format!("\"{}\"", policy.get(e.id).as_str());
            out.push_str(&format!("{key:<key_w$} = {value:<7}  # {}\n", e.help));
        }
    }
    out
}

/// The catalogue as a Markdown table, one per group, for the docs: what is
/// listed there is what the code has, and a test says when they part.
pub fn markdown_table() -> String {
    let mut out = String::new();
    for cat in Category::ALL {
        out.push_str(&format!("**{}** — {}\n\n", cat.title(), cat.about()));
        out.push_str(
            "| Name | Default when turned on | What it does | Refused flags |\n|---|---|---|---|\n",
        );
        for e in CATALOG.iter().filter(|e| e.category == cat) {
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
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_is_consistent() {
        let ids: Vec<&str> = CATALOG.iter().map(|e| e.id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(ids.len(), unique.len(), "duplicate ids");
        for e in CATALOG {
            assert!(!e.help.is_empty());
            assert!(!e.program().is_empty());
            assert!(Category::ALL.contains(&e.category));
            assert_ne!(e.on, Permission::Off, "{}", e.id);
        }
        // the editor's own first, in a group of their own with the setting
        // on where commands run, and the rest are commands
        assert_eq!(CATALOG[0].id, READ_FILES);
        assert_eq!(
            (CATALOG[0].kind, CATALOG[0].on),
            (Kind::Read, Permission::Allow)
        );
        for id in [READ_FILES, EDIT_FILES, CREATE_FILES] {
            let e = find(id).unwrap();
            assert_eq!(e.category, Category::Editor, "{id}");
            assert!(!e.is_command());
        }
        let sub = find(SUBFOLDERS).unwrap();
        assert_eq!(
            (sub.category, sub.kind),
            (Category::Editor, Kind::Subfolders)
        );
        assert!(CATALOG
            .iter()
            .filter(|e| e.category != Category::Editor)
            .all(Entry::is_command));
        // every group has something in it
        for c in Category::ALL {
            assert!(CATALOG.iter().any(|e| e.category == c), "{}", c.title());
        }
        assert_eq!(
            find("git diff").map(|e| e.prefix()),
            Some(vec!["diff".into()])
        );
        assert_eq!(find("ls").map(|e| e.prefix()), Some(vec![]));
        assert!(find("rm").is_none());
        assert!(find("git push").is_none());
        // what changes things, or reaches out, asks by default; what only
        // looks allows
        assert_eq!(find("git commit").unwrap().on, Permission::Ask);
        assert_eq!(find("make").unwrap().on, Permission::Ask);
        assert_eq!(find("curl").unwrap().on, Permission::Ask);
        assert_eq!(find("wget").unwrap().category, Category::Network);
        assert_eq!(find("git diff").unwrap().on, Permission::Allow);
        assert_eq!(find("cat").unwrap().on, Permission::Allow);
        assert_eq!(find(EDIT_FILES).unwrap().on, Permission::Ask);
        // a folder is a change: mkdir asks, and comes with the coreutils
        let mkdir = find("mkdir").unwrap();
        assert_eq!(
            (mkdir.on, mkdir.coreutils, mkdir.category),
            (Permission::Ask, true, Category::Files)
        );
        assert_eq!(sub.resolve(), Some(PathBuf::new()));
        assert_eq!(find(READ_FILES).unwrap().resolve(), Some(PathBuf::new()));
    }

    #[test]
    fn the_policy_keeps_the_one_rule_and_drops_what_it_cannot_take() {
        let mut p = Policy::from_pairs([
            ("git diff", Permission::Allow),
            ("rm -rf", Permission::Allow),
            (EDIT_FILES, Permission::Allow),
            ("git commit", Permission::Off),
        ]);
        // unknown dropped, off dropped, allow on an edit is the user's call,
        // and editing brought reading with it
        assert_eq!(p.get("git diff"), Permission::Allow);
        assert_eq!(p.get("rm -rf"), Permission::Off);
        assert_eq!(p.get("git commit"), Permission::Off);
        assert_eq!(p.get(EDIT_FILES), Permission::Allow);
        assert_eq!(p.get(READ_FILES), Permission::Allow);
        assert!(p.reads() && p.edits() && !p.creates());
        assert_eq!(p.counts(), (3, 0));
        let ids: Vec<&str> = p.entries().iter().map(|(e, _)| e.id).collect();
        assert_eq!(ids, vec![READ_FILES, EDIT_FILES, "git diff"]);
        assert_eq!(p.commands().len(), 1);
        // reading off takes editing and creating with it
        p.set(CREATE_FILES, Permission::Ask);
        assert_eq!(p.counts(), (4, 1));
        p.set(READ_FILES, Permission::Off);
        assert!(!p.reads() && !p.edits() && !p.creates());
        assert_eq!(p.get("git diff"), Permission::Allow);
        // and reading to ask is a choice like any other
        p.set(READ_FILES, Permission::Ask);
        assert_eq!(p.get(READ_FILES), Permission::Ask);
        // the file form is plain strings, and round-trips
        let pairs = p.pairs();
        assert_eq!(pairs.get("git diff"), Some(&Permission::Allow));
        assert_eq!(Policy::from_pairs(pairs), p);
        assert!(Policy::default().is_empty());
        p.set("git diff", Permission::Off);
        p.set(READ_FILES, Permission::Off);
        assert!(p.is_empty());
    }

    #[test]
    fn subfolders_is_yes_or_no_and_keeps_its_old_name() {
        let mut p = Policy::default();
        p.set(SUBFOLDERS, Permission::Ask);
        assert_eq!(p.get(SUBFOLDERS), Permission::Allow);
        assert!(p.subfolders());
        // not a command, and on by itself it turns nothing else on
        assert!(p.commands().is_empty() && !p.reads());
        // a tools.toml from before the rename still says it
        let old = Policy::from_pairs([(SUBFOLDERS_OLD, Permission::Allow)]);
        assert!(old.subfolders());
        assert_eq!(old.pairs().keys().collect::<Vec<_>>(), vec![SUBFOLDERS]);
        assert_eq!(find(SUBFOLDERS_OLD).map(|e| e.id), Some(SUBFOLDERS));
    }

    #[test]
    fn a_call_matches_the_longest_id_among_the_ones_that_are_on() {
        let p = Policy::from_pairs([
            ("git diff", Permission::Allow),
            ("git log", Permission::Ask),
            ("ls", Permission::Allow),
            ("cargo check", Permission::Allow),
            (READ_FILES, Permission::Allow),
        ]);
        let toks = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
        let (e, n) = match_call(&p, &toks("git diff --stat")).unwrap();
        assert_eq!((e.id, n), ("git diff", 2));
        let (e, n) = match_call(&p, &toks("ls -la src")).unwrap();
        assert_eq!((e.id, n), ("ls", 1));
        assert!(match_call(&p, &toks("git status")).is_none());
        assert!(match_call(&p, &toks("cargo")).is_none());
        assert!(match_call(&p, &toks("rm -rf .")).is_none());
        assert!(match_call(&p, &toks("read files")).is_none());
        assert!(match_call(&p, &[]).is_none());
    }

    #[test]
    fn arguments_stay_inside_and_off_the_deny_list() {
        let git = find("git diff").unwrap();
        for ok in [
            "--stat",
            "HEAD~2..HEAD",
            "src/main.rs",
            "--",
            "-n",
            "5",
            "--format=%H %s",
            "origin/main",
            "@~1",
            "*.rs",
        ] {
            assert_eq!(check_arg(git, ok), Ok(()), "{ok}");
        }
        for bad in [
            "/etc/passwd",
            "../x",
            "src/../../x",
            "~/.ssh/id_rsa",
            "C:\\Windows",
            "c:/x",
            "..\\x",
            "--output=/tmp/x",
            "--output",
            "--exec-path",
            "--git-dir=../x",
            "a\nb",
        ] {
            assert!(check_arg(git, bad).is_err(), "{bad}");
        }
        // a value inside a flag is a path too
        assert!(check_arg(git, "--relative=../x").is_err());
        assert_eq!(check_arg(git, "--relative=src"), Ok(()));
        // the deny list is the command's own
        let find_ = find("find").unwrap();
        assert!(check_arg(find_, "-exec").is_err());
        assert!(check_arg(find_, "-delete").is_err());
        assert_eq!(check_arg(find_, "-name"), Ok(()));
        assert_eq!(check_arg(git, "-exec"), Ok(()));
        assert!(check_arg(find("make").unwrap(), "--eval=x").is_err());
        assert!(check_arg(find("cargo test").unwrap(), "--config").is_err());
        // a URL is not a path, and what would send a file away is refused
        let curl = find("curl").unwrap();
        assert_eq!(check_arg(curl, "https://example.com/a/b"), Ok(()));
        assert_eq!(check_arg(curl, "-sL"), Ok(()));
        assert_eq!(check_arg(curl, "-o"), Ok(()));
        for bad in [
            "-d",
            "--data-binary",
            "--data-binary=@.env",
            "-F",
            "-T",
            "-K",
            "--json",
        ] {
            assert!(check_arg(curl, bad).is_err(), "{bad}");
        }
        let wget = find("wget").unwrap();
        assert!(check_arg(wget, "--post-file=x").is_err());
        assert!(check_arg(wget, "-i").is_err());
        assert_eq!(check_arg(wget, "-q"), Ok(()));
    }

    #[test]
    fn the_file_lists_everything_and_reads_back_as_the_policy() {
        let p = Policy::from_pairs([
            (READ_FILES, Permission::Allow),
            (EDIT_FILES, Permission::Ask),
            ("curl", Permission::Ask),
        ]);
        let text = render_tools_file(&p, 5);
        assert!(text.starts_with("# What the model may do"), "{text}");
        assert!(text.contains("max_steps = 5"), "{text}");
        // every entry, in its group, off included
        for e in CATALOG {
            assert!(text.contains(&format!("\"{}\"", e.id)), "{}", e.id);
        }
        for c in Category::ALL {
            assert!(
                text.contains(&format!("# {} — ", c.title())),
                "{}",
                c.title()
            );
        }
        assert!(text.contains("\"ls\""), "{text}");
        let ls = text.lines().find(|l| l.starts_with("\"ls\"")).unwrap();
        assert!(
            ls.contains("= \"off\"") && ls.contains("# list a folder"),
            "{ls}"
        );
        // it parses, and what it says is the policy it came from
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tools.toml");
        moon_core::ToolsFile::write_text(&path, &text).unwrap();
        let back = moon_core::ToolsFile::load(&path).unwrap().unwrap();
        assert_eq!(back.max_steps, 5);
        assert_eq!(Policy::from_pairs(back.permissions), p);
    }

    /// `docs/tools.md` carries `markdown_table()` between two markers.
    /// `UPDATE_DOCS=1 cargo test -p moon-agent docs` rewrites it there.
    #[test]
    fn the_docs_carry_the_catalogue_as_it_is() {
        let table = markdown_table();
        for e in CATALOG {
            assert!(table.contains(&format!("| `{}` |", e.id)), "{}", e.id);
        }
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/tools.md");
        let docs = std::fs::read_to_string(&path).expect("docs/tools.md");
        let (start, end) = ("<!-- CATALOG:START -->\n", "<!-- CATALOG:END -->");
        let a = docs.find(start).expect("start marker") + start.len();
        let b = docs.find(end).expect("end marker");
        let body = format!("\n{table}");
        if std::env::var_os("UPDATE_DOCS").is_some() {
            let new = format!("{}{body}{}", &docs[..a], &docs[b..]);
            std::fs::write(&path, new).unwrap();
            return;
        }
        assert_eq!(
            &docs[a..b],
            body,
            "docs/tools.md is out of date: UPDATE_DOCS=1 cargo test -p moon-agent docs"
        );
    }

    #[test]
    fn resolving_finds_what_is_on_path_and_nothing_else() {
        // whatever the machine, something that is not there is not found
        assert!(resolve("moon-no-such-program-xyz", false).is_none());
        assert!(resolve("", false).is_none());
        // and the coreutils, where they are on `PATH`
        #[cfg(unix)]
        {
            let ls = resolve("ls", true).expect("ls on PATH");
            assert!(ls.is_absolute() && ls.ends_with("ls"));
            assert_eq!(find("ls").unwrap().resolve(), Some(ls));
        }
    }
}
