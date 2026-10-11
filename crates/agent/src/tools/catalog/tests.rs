use super::*;
use moon_core::config::ids::OTHER_PROGRAMS;

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
    // what runs anything, from anywhere, stays out: a bare container, a
    // package fetched and run
    for id in [
        "docker run",
        "docker exec",
        "npx",
        "npm exec",
        "pnpm dlx",
        "pnpm exec",
    ] {
        assert!(find(id).is_none(), "{id}");
    }
    // the compose looks freely, starts and runs inside asking, and `up`
    // only detached
    assert_eq!(find("docker compose logs").unwrap().on, Permission::Allow);
    assert!(find("docker compose logs").unwrap().deny.contains(&"-f"));
    assert_eq!(find("docker compose exec").unwrap().on, Permission::Ask);
    let up = find("docker compose up").unwrap();
    assert_eq!(
        (up.category, up.on, up.needs),
        (Category::Docker, Permission::Ask, &["-d", "--detach"][..])
    );
    assert!(find("docker compose down").unwrap().needs.is_empty());
    assert_eq!(find("pnpm install").unwrap().on, Permission::Ask);
    assert_eq!(find("python3").unwrap().category, Category::Stack);
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
    // a file from before the rename still says it
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
    // a URL fetches over http only: `file://` would read the machine
    for bad in [
        "file:///etc/passwd",
        "FILE:///etc/passwd",
        "file://localhost/etc/passwd",
        "ftp://example.com/x",
        "scp://host/etc/passwd",
        "--url=file:///etc/passwd",
    ] {
        assert!(check_arg(curl, bad).is_err(), "{bad}");
    }
    assert!(check_arg(wget, "ftp://example.com/x").is_err());
    assert_eq!(check_arg(curl, "HTTP://example.com"), Ok(()));
}

#[test]
fn a_path_behind_file_is_still_a_path() {
    let cat = find("cat").unwrap();
    for bad in [
        "file:/etc/passwd",
        "file:///etc/passwd",
        "file:~/.ssh/id_rsa",
        "file:../x",
        "file:file:/x",
        "file://host/x",
    ] {
        assert!(check_arg(cat, bad).is_err(), "{bad}");
    }
    assert_eq!(check_arg(cat, "file:src/x"), Ok(()));
    // the password sources of openssl are the same
    let enc = find("openssl pkey").unwrap();
    assert!(check_arg(enc, "file:/Users/x/.ssh/key").is_err());
    assert_eq!(check_arg(enc, "file:keys/pass.txt"), Ok(()));
    // multibyte first characters do not break the scheme check
    assert_eq!(check_arg(cat, "ñandú.txt"), Ok(()));
}

#[test]
fn models_go_by_subcommand_and_all_ask() {
    let models: Vec<&Entry> = CATALOG
        .iter()
        .filter(|e| e.category == Category::Models)
        .collect();
    assert!(!models.is_empty());
    for e in &models {
        assert_eq!(e.on, Permission::Ask, "{}", e.id);
        let section = if e.program() == "ollama" {
            Section::Ollama
        } else {
            Section::OpenAi
        };
        assert_eq!(e.section, Some(section), "{}", e.id);
    }
    // what blocks, uploads or spends on its own is not in the catalogue
    for id in [
        "ollama",
        "openai",
        "ollama serve",
        "ollama push",
        "ollama create",
        "openai api files.create",
        "openai api audio.transcriptions.create",
        "openai api fine_tuning.jobs.create",
        "openai tools",
    ] {
        assert!(find(id).is_none(), "{id}");
    }
    let chat = find("openai api chat.completions.create").unwrap();
    for bad in ["-b", "--api-base=https://evil.example", "--proxy"] {
        assert!(check_arg(chat, bad).is_err(), "{bad}");
    }
    assert_eq!(check_arg(chat, "-t"), Ok(()));
    let pull = find("ollama pull").unwrap();
    assert!(check_arg(pull, "--insecure").is_err());
    assert_eq!(
        check_arg(pull, "hf.co/unsloth/Qwen3-8B-GGUF:Q4_K_M"),
        Ok(())
    );
    let mut p = Policy::default();
    p.set("ollama rm", Permission::Ask);
    let toks: Vec<String> = ["ollama", "rm", "llama3:8b"].map(String::from).into();
    let (e, n) = match_call(&p, &toks).unwrap();
    assert_eq!((e.id, n), ("ollama rm", 2));
}

#[test]
fn openssl_goes_by_subcommand_and_loads_nothing_from_elsewhere() {
    assert!(find("openssl").is_none());
    assert!(find("openssl enc").is_none());
    for id in [
        "openssl rand",
        "openssl dgst",
        "openssl base64",
        "openssl x509",
        "openssl verify",
        "shasum",
        "sha256sum",
        "uuidgen",
    ] {
        let e = find(id).unwrap();
        assert_eq!(
            (e.category, e.on),
            (Category::Crypto, Permission::Allow),
            "{id}"
        );
    }
    for id in ["openssl pkey", "openssl genpkey", "openssl req", "htpasswd"] {
        let e = find(id).unwrap();
        assert_eq!(
            (e.category, e.on),
            (Category::Crypto, Permission::Ask),
            "{id}"
        );
    }
    let s_client = find("openssl s_client").unwrap();
    assert_eq!(
        (s_client.category, s_client.on),
        (Category::Network, Permission::Ask)
    );
    for id in ["openssl rand", "openssl req", "openssl s_client"] {
        let e = find(id).unwrap();
        for bad in [
            "-engine",
            "-provider",
            "-provider-path",
            "-config",
            "-writerand",
        ] {
            assert!(check_arg(e, bad).is_err(), "{id} {bad}");
        }
    }
    let rand = find("openssl rand").unwrap();
    assert_eq!(check_arg(rand, "-base64"), Ok(()));
    assert_eq!(check_arg(rand, "12"), Ok(()));
    // a key never goes out on stdout
    assert_eq!(find("openssl genpkey").unwrap().needs, &["-out"][..]);
}

#[test]
fn a_literal_flag_takes_a_value_that_is_not_a_path() {
    let req = find("openssl req").unwrap();
    let args = |s: &[&str]| s.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    assert_eq!(
        check_args(
            req,
            &args(&[
                "-x509",
                "-nodes",
                "-subj",
                "/CN=localhost",
                "-out",
                "cert.pem"
            ])
        ),
        Ok(())
    );
    // only the value right after it, and never elsewhere
    assert!(check_args(req, &args(&["-subj", "/CN=x", "/etc/passwd"])).is_err());
    assert!(check_args(req, &args(&["-out", "/tmp/cert.pem"])).is_err());
    // and still without control characters
    assert!(check_args(req, &args(&["-subj", "/CN=a\nb"])).is_err());
    // other entries have no literal flags
    let x509 = find("openssl x509").unwrap();
    assert!(check_args(x509, &args(&["-subj", "/CN=x"])).is_err());
}

#[test]
fn the_stack_and_models_are_in_sections_and_nothing_else_is() {
    for e in CATALOG {
        assert_eq!(
            e.section.is_some(),
            matches!(e.category, Category::Stack | Category::Models),
            "{}",
            e.id
        );
    }
    // each section in one run, in the order the panel shows them
    let order: Vec<Section> =
        CATALOG
            .iter()
            .filter_map(|e| e.section)
            .fold(Vec::new(), |mut v, s| {
                if v.last() != Some(&s) {
                    v.push(s);
                }
                v
            });
    assert_eq!(
        order,
        [
            Section::Make,
            Section::Rust,
            Section::Node,
            Section::Python,
            Section::Ollama,
            Section::OpenAi
        ]
    );
    assert_eq!(find("make").unwrap().section, Some(Section::Make));
    assert_eq!(find("pnpm list").unwrap().section, Some(Section::Node));
    assert_eq!(find("pip install").unwrap().section, Some(Section::Python));
    // an agent file titles each one under the group
    let text = crate::AgentFile::default().to_toml();
    let rust = text.find("\n# Rust\n").expect("rust title");
    assert!(text.find("\n# Stack — ").unwrap() < rust, "{text}");
    assert!(
        text[rust..].starts_with("\n# Rust\n\"cargo check\""),
        "{text}"
    );
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
    // a Windows checkout may bring the file with CRLF line endings
    let docs = std::fs::read_to_string(&path)
        .expect("docs/tools.md")
        .replace("\r\n", "\n");
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

#[test]
fn other_programs_is_ask_or_off_and_turns_run_command_on() {
    let mut p = Policy::default();
    assert!(!p.runs());
    p.set(OTHER_PROGRAMS, Permission::Allow);
    assert_eq!(p.get(OTHER_PROGRAMS), Permission::Ask);
    assert!(p.others() && p.runs());
    // not a command of the catalogue: it matches nothing by itself
    assert!(p.commands().is_empty());
    let e = find(OTHER_PROGRAMS).unwrap();
    assert_eq!(
        (e.category, e.kind, e.on),
        (Category::Editor, Kind::Other, Permission::Ask)
    );
    assert!(!e.is_command());
    p.set(OTHER_PROGRAMS, Permission::Off);
    assert!(!p.runs());
}

#[test]
fn a_program_is_a_name_and_never_a_shell() {
    for ok in ["rm", "rustup", "gh", "node", "python3.12"] {
        assert_eq!(check_program(ok), Ok(()), "{ok}");
    }
    for bad in [
        "sh",
        "bash",
        "zsh",
        "Cmd.exe",
        "powershell",
        "pwsh",
        "env",
        "xargs",
        "sudo",
        "timeout",
        "/bin/rm",
        "./x",
        "bin/x",
        "..\\x",
        "-rf",
        "c:x",
    ] {
        assert!(check_program(bad).is_err(), "{bad}");
    }
}

#[test]
fn a_call_outside_the_catalogue_takes_its_family_s_rules() {
    let toks = |s: &str| s.split(' ').map(String::from).collect::<Vec<_>>();
    // the entry whose words open it, off or on
    let (e, whole) = rules_for(&toks("docker compose up api")).unwrap();
    assert_eq!((e.id, whole), ("docker compose up", true));
    // or the one of the program that refuses least: `logs` refuses `-f`,
    // which `docker compose -f other.yml run` needs
    let (e, whole) = rules_for(&toks("docker compose -f x.yml run api")).unwrap();
    assert!(!whole);
    assert!(
        e.deny.contains(&"--host") && !e.deny.contains(&"-f"),
        "{}",
        e.id
    );
    let (e, _) = rules_for(&toks("git push")).unwrap();
    assert!(e.deny.contains(&"--exec-path"));
    assert!(rules_for(&toks("rustup show")).is_none());
    assert!(rules_for(&[]).is_none());
}

#[test]
fn moving_and_deleting_need_reading_like_every_change() {
    use moon_core::config::ids::{DELETE_FILES, MOVE_FILES};
    let mut p = Policy::default();
    p.set(MOVE_FILES, Permission::Ask);
    assert!(p.reads() && p.moves() && p.changes_files());
    p.set(DELETE_FILES, Permission::Allow);
    assert!(p.deletes() && p.writes_unseen());
    p.set(READ_FILES, Permission::Off);
    assert!(!p.moves() && !p.deletes() && p.is_empty());
    for id in [MOVE_FILES, DELETE_FILES] {
        let e = find(id).unwrap();
        assert_eq!(
            (e.category, e.on),
            (Category::Editor, Permission::Ask),
            "{id}"
        );
        assert!(!e.is_command());
    }
}
