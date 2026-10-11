//! What the model may be allowed to do, in one catalogue: moon's own file
//! tools and a fixed list of commands, in groups, each with a permission
//! the agent's file sets: off, ask or allow. Outside it, only `other
//! programs`, which always asks; no shell is ever involved: a command runs
//! as a program with its arguments as a list, from a folder inside the
//! project.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use moon_core::config::ids::{
    CREATE_FILES, DELETE_FILES, EDIT_FILES, MOVE_FILES, OTHER_PROGRAMS, READ_FILES, SUBFOLDERS,
    SUBFOLDERS_OLD,
};
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
    /// Secrets, hashes, keys and certificates: `openssl` by subcommand.
    Crypto,
    /// Language models, local and in the cloud: `ollama` and `openai` by
    /// subcommand, in sections.
    Models,
    Network,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Editor,
        Category::Files,
        Category::Git,
        Category::Stack,
        Category::Docker,
        Category::Crypto,
        Category::Models,
        Category::Network,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Editor => "Editor",
            Category::Files => "Files",
            Category::Git => "Git",
            Category::Stack => "Stack",
            Category::Docker => "Docker",
            Category::Crypto => "Crypto",
            Category::Models => "Models",
            Category::Network => "Network",
        }
    }

    /// One line on what the group is: the panel's hint under its title, and
    /// the comment over it in an agent file.
    pub fn about(self) -> &'static str {
        match self {
            Category::Editor => {
                "moon's own tools, and how the model's calls run: files read, changed, moved and deleted with your ok, commands in subfolders, other programs, the step limit"
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
            Category::Crypto => {
                "secrets, hashes, keys and certificates: what only prints allows, what makes a key asks; what they print goes to the model"
            }
            Category::Models => {
                "the models on this machine and in your OpenAI account: everything asks, what a model answers goes to the model"
            }
            Category::Network => {
                "what a command fetches goes to the model; flags that would send a file are refused"
            }
        }
    }
}

/// Inside `Stack`, the language an entry belongs to, and inside `Models`
/// the provider: the panel, an agent file and the docs set each one apart
/// under its title.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Section {
    Make,
    Rust,
    Node,
    Python,
    Ollama,
    OpenAi,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Make => "Make",
            Section::Rust => "Rust",
            Section::Node => "Node",
            Section::Python => "Python",
            Section::Ollama => "Ollama",
            Section::OpenAi => "OpenAI",
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
    /// `move_file`.
    Move,
    /// `delete_file`.
    Delete,
    /// Not a program: a call may carry a `dir`, a folder below the project
    /// root to run in, never one above it. Off or allow, nothing to ask.
    Subfolders,
    /// Not a program: any program the catalogue does not list may be
    /// called, each call shown to the user first. Off or ask, never allow.
    Other,
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
    /// Its section inside `Stack` or `Models`; the other groups are one
    /// list.
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
    /// Flags whose value is not a path, however it looks: `-subj
    /// /CN=localhost` for `openssl req`.
    pub literal: &'static [&'static str],
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
        literal: &[],
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

    /// The same entry, with `flags` taking a value that is not a path.
    const fn literal(mut self, flags: &'static [&'static str]) -> Entry {
        self.literal = flags;
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

/// What loads code or a configuration from elsewhere, and what writes the
/// seed file back: the same for every subcommand.
const OPENSSL_DENY: &[&str] = &[
    "-engine",
    "-provider",
    "-provider-path",
    "-propquery",
    "-config",
    "-writerand",
];

const fn crypto(id: &'static str, on: Permission, help: &'static str) -> Entry {
    entry(
        id,
        Category::Crypto,
        Kind::Command,
        on,
        OPENSSL_DENY,
        false,
        help,
    )
}

/// Not to another registry over plain HTTP.
const OLLAMA_DENY: &[&str] = &["--insecure"];

/// The key goes to OpenAI only: never to another host or through a proxy.
const OPENAI_DENY: &[&str] = &[
    "-b",
    "--api-base",
    "-p",
    "--proxy",
    "--azure-endpoint",
    "--api-type",
];

/// Every one asks: what only lists still reaches a daemon or your account,
/// and the rest downloads, deletes or spends credit.
const fn model(id: &'static str, section: Section, help: &'static str) -> Entry {
    let deny = match section {
        Section::OpenAi => OPENAI_DENY,
        _ => OLLAMA_DENY,
    };
    let mut e = entry(
        id,
        Category::Models,
        Kind::Command,
        Permission::Ask,
        deny,
        false,
        help,
    );
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
    editor(
        MOVE_FILES,
        Kind::Move,
        Permission::Ask,
        "a rename you apply or skip; never over a file that exists",
    ),
    editor(
        DELETE_FILES,
        Kind::Delete,
        Permission::Ask,
        "the file shown as removed, you apply or skip; only one it has read",
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
    entry(
        OTHER_PROGRAMS,
        Category::Editor,
        Kind::Other,
        Permission::Ask,
        &[],
        false,
        "any program not listed here, every call shown to you first; there is no allow",
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
    crypto(
        "openssl rand",
        Permission::Allow,
        "random bytes, `-base64 12` or `-hex 32`: a secret or a token",
    ),
    crypto(
        "openssl dgst",
        Permission::Allow,
        "a file's hash, `-sha256`, or its HMAC",
    ),
    crypto(
        "openssl base64",
        Permission::Allow,
        "encode to base64, or decode with `-d`",
    ),
    crypto(
        "openssl x509",
        Permission::Allow,
        "read a certificate: `-noout -text`, `-dates`, `-subject`",
    ),
    crypto(
        "openssl verify",
        Permission::Allow,
        "check a certificate against its chain",
    ),
    crypto(
        "openssl pkey",
        Permission::Ask,
        "read a key, or its public half with `-pubout`; a private one goes to the model",
    ),
    crypto(
        "openssl genpkey",
        Permission::Ask,
        "make a private key, into a file: `-out` is required",
    )
    .needing(&["-out"]),
    crypto(
        "openssl req",
        Permission::Ask,
        "a CSR, or a self-signed certificate with `-x509`",
    )
    .literal(&["-subj"]),
    entry(
        "shasum",
        Category::Crypto,
        Kind::Command,
        Permission::Allow,
        &[],
        false,
        "a file's hash, `-a 256`, or check a list with `-c`",
    ),
    entry(
        "sha256sum",
        Category::Crypto,
        Kind::Command,
        Permission::Allow,
        &[],
        false,
        "a file's SHA-256, or check a list with `-c`",
    ),
    entry(
        "uuidgen",
        Category::Crypto,
        Kind::Command,
        Permission::Allow,
        &[],
        false,
        "a random UUID",
    ),
    entry(
        "htpasswd",
        Category::Crypto,
        Kind::Command,
        Permission::Ask,
        &[],
        false,
        "a basic-auth line, `-nbB user password`, or into a file",
    ),
    model("ollama list", Section::Ollama, "the models installed"),
    model("ollama ps", Section::Ollama, "the models loaded in memory"),
    model(
        "ollama show",
        Section::Ollama,
        "a model's parameters, template and Modelfile",
    ),
    model(
        "ollama run",
        Section::Ollama,
        "one prompt and its answer: `ollama run qwen3 \"…\"`",
    ),
    model(
        "ollama pull",
        Section::Ollama,
        "download a model, often gigabytes",
    ),
    model("ollama stop", Section::Ollama, "unload a model from memory"),
    model(
        "ollama cp",
        Section::Ollama,
        "copy a model under another name",
    ),
    model(
        "ollama rm",
        Section::Ollama,
        "delete a model from the machine, outside the project",
    ),
    model(
        "openai api models.list",
        Section::OpenAi,
        "the models of your account, your fine-tunes included",
    ),
    model(
        "openai api models.retrieve",
        Section::OpenAi,
        "a model's details: whether it exists and whose it is",
    ),
    model(
        "openai api models.delete",
        Section::OpenAi,
        "delete one of your fine-tuned models",
    ),
    model(
        "openai api files.list",
        Section::OpenAi,
        "the files uploaded to your account",
    ),
    model(
        "openai api files.retrieve",
        Section::OpenAi,
        "an uploaded file's size, purpose and status",
    ),
    model(
        "openai api files.delete",
        Section::OpenAi,
        "delete an uploaded file",
    ),
    model(
        "openai api fine_tuning.jobs.list",
        Section::OpenAi,
        "the fine-tuning jobs, running and past",
    ),
    model(
        "openai api fine_tuning.jobs.retrieve",
        Section::OpenAi,
        "a job's status and the model it produced",
    ),
    model(
        "openai api fine_tuning.jobs.list_events",
        Section::OpenAi,
        "a job's log: loss, errors",
    ),
    model(
        "openai api fine_tuning.jobs.cancel",
        Section::OpenAi,
        "stop a fine-tuning job",
    ),
    model(
        "openai api chat.completions.create",
        Section::OpenAi,
        "one completion, `-m model -g user \"…\"`: it spends credit",
    ),
    network(
        "openssl s_client",
        OPENSSL_DENY,
        "the certificate a host serves: `-connect host:443`",
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
/// is off. The one rule it keeps on its own: every change to files needs
/// reading, so turning editing, creating, moving or deleting on turns `read
/// files` on, and turning `read files` off turns them off.
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
            // what the catalogue does not list is always seen first
            (Kind::Other, Permission::Allow) => Permission::Ask,
            (_, p) => p,
        };
        if perm == Permission::Off {
            self.map.remove(entry.id);
        } else {
            self.map.insert(entry.id, perm);
        }
        match entry.kind {
            Kind::Edit | Kind::Create | Kind::Move | Kind::Delete if perm != Permission::Off => {
                if !self.allows(READ_FILES) {
                    self.map.insert(READ_FILES, Permission::Allow);
                }
            }
            Kind::Read if perm == Permission::Off => {
                for id in [EDIT_FILES, CREATE_FILES, MOVE_FILES, DELETE_FILES] {
                    self.map.remove(id);
                }
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

    pub fn moves(&self) -> bool {
        self.allows(MOVE_FILES)
    }

    pub fn deletes(&self) -> bool {
        self.allows(DELETE_FILES)
    }

    /// Something on disk may change: an edit, a new file, a move or a
    /// delete.
    pub fn changes_files(&self) -> bool {
        self.edits() || self.creates() || self.moves() || self.deletes()
    }

    pub fn subfolders(&self) -> bool {
        self.allows(SUBFOLDERS)
    }

    /// Programs outside the catalogue may be asked for.
    pub fn others(&self) -> bool {
        self.allows(OTHER_PROGRAMS)
    }

    /// `run_command` has something to run: a command that is on, or any
    /// other program.
    pub fn runs(&self) -> bool {
        self.others() || !self.commands().is_empty()
    }

    /// A change lands without being shown first: editing, creating, moving
    /// or deleting at `allow`.
    pub fn writes_unseen(&self) -> bool {
        [EDIT_FILES, CREATE_FILES, MOVE_FILES, DELETE_FILES]
            .iter()
            .any(|id| self.get(id) == Permission::Allow)
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

/// Programs never run as an `other program`, even asking: a shell, and
/// what runs another program in its place, which would take it past the
/// checks of its arguments and the deny list of its family.
pub const NEVER_RUN: &[&str] = &[
    "sh",
    "bash",
    "zsh",
    "fish",
    "dash",
    "ksh",
    "csh",
    "tcsh",
    "nu",
    "pwsh",
    "powershell",
    "cmd",
    "sudo",
    "doas",
    "su",
    "env",
    "xargs",
    "nohup",
    "nice",
    "time",
    "timeout",
    "watch",
    "exec",
    "command",
    "eval",
    "script",
    "ssh",
];

/// The catalogue's say over a call made as an `other program`: the entry
/// whose words open it, even if it is off, gives its deny list, its needs
/// and its literal flags; failing that, the entry of the same program that
/// refuses least gives its deny list, so `git push --exec-path` is refused
/// as `git diff --exec-path` is. `None` for a program the catalogue does not
/// know.
pub fn rules_for(tokens: &[String]) -> Option<(&'static Entry, bool)> {
    let program = tokens.first()?;
    let whole = CATALOG
        .iter()
        .filter(|e| e.is_command())
        .filter(|e| {
            let n = e.words().count();
            tokens.len() >= n && e.words().zip(tokens).all(|(w, t)| w == t)
        })
        .max_by_key(|e| e.words().count());
    if let Some(e) = whole {
        return Some((e, true));
    }
    CATALOG
        .iter()
        .filter(|e| e.is_command() && e.program() == program)
        .min_by_key(|e| e.deny.len())
        .map(|e| (e, false))
}

/// Why a program cannot be called as an `other program`: a path rather
/// than a name, or one of `NEVER_RUN`.
pub fn check_program(program: &str) -> Result<(), String> {
    if program.starts_with('-') || program.contains(['/', '\\']) || program.contains(':') {
        return Err(format!(
            "`{program}`: name a program on PATH, not a path or a flag"
        ));
    }
    let lower = program.to_ascii_lowercase();
    let base = lower
        .strip_suffix(".exe")
        .or_else(|| lower.strip_suffix(".cmd"))
        .or_else(|| lower.strip_suffix(".bat"))
        .unwrap_or(&lower);
    if NEVER_RUN.contains(&base) {
        return Err(format!(
            "`{program}` is never run: no shell, and nothing that runs another program"
        ));
    }
    Ok(())
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
    if entry.category == Category::Network && value.contains("://") {
        let lower = value.to_ascii_lowercase();
        if !(lower.starts_with("http://") || lower.starts_with("https://")) {
            return Err(format!("`{value}`: only http:// and https:// URLs"));
        }
    }
    check_path_like(value)
}

/// Every argument of a call, as `check_arg` takes each one; the one after
/// a flag of the entry's `literal` is not a path, so `-subj /CN=localhost`
/// is let through.
pub fn check_args(entry: &Entry, args: &[String]) -> Result<(), String> {
    let mut literal = false;
    for arg in args {
        if literal {
            if arg.chars().any(char::is_control) {
                return Err("arguments cannot contain control characters".into());
            }
        } else {
            check_arg(entry, arg)?;
        }
        literal = entry.literal.contains(&arg.as_str());
    }
    Ok(())
}

/// What is behind a `file:` prefix, if the value has one: `file:keys/pass`
/// is `keys/pass`, and `file:///etc/passwd` is `///etc/passwd`, absolute as
/// every `file://` URL is.
pub fn behind_file_scheme(v: &str) -> Option<&str> {
    let mut rest = v;
    while rest
        .get(..5)
        .is_some_and(|s| s.eq_ignore_ascii_case("file:"))
    {
        rest = &rest[5..];
    }
    (rest.len() != v.len()).then_some(rest)
}

fn check_path_like(v: &str) -> Result<(), String> {
    // `-passin file:/…`, `curl file:///…`: a path behind the scheme is one
    if let Some(rest) = behind_file_scheme(v) {
        return check_path_like(rest).map_err(|_| format!("`{v}` is outside the project"));
    }
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
