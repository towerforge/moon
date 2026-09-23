//! The boundary, case by case, on a real tree in a `tempdir`.

use std::fs;
use std::path::Path;

use super::*;

fn tree() -> (tempfile::TempDir, Sandbox) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    fs::create_dir_all(r.join("src/deep")).unwrap();
    fs::create_dir_all(r.join(".git/hooks")).unwrap();
    fs::create_dir_all(r.join(".github/workflows")).unwrap();
    fs::create_dir_all(r.join("target")).unwrap();
    fs::write(r.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(r.join("src/deep/x.rs"), "x\n").unwrap();
    fs::write(r.join("src/win.rs"), "\u{feff}a\r\nb\r\n").unwrap();
    fs::write(r.join("README.md"), "# hi\n").unwrap();
    fs::write(r.join(".env"), "SECRET=1\n").unwrap();
    fs::write(r.join(".git/config"), "[core]\n").unwrap();
    fs::write(r.join(".github/workflows/ci.yml"), "on: push\n").unwrap();
    fs::write(r.join("logo.png"), [0x89, b'P', b'N', b'G', 0, 0]).unwrap();
    fs::write(r.join("big.txt"), "x".repeat(300)).unwrap();
    let sb = Sandbox::new(r, 200, &[".github/workflows/**".into()]).unwrap();
    (dir, sb)
}

fn denied(r: Result<impl std::fmt::Debug, Denied>) -> Denied {
    match r {
        Ok(v) => panic!("expected a refusal, got {v:?}"),
        Err(e) => e,
    }
}

#[test]
fn lexical_rules_need_no_disk() {
    assert_eq!(
        Sandbox::components("src/main.rs").unwrap(),
        vec!["src", "main.rs"]
    );
    assert_eq!(
        Sandbox::components("./src//main.rs").unwrap(),
        vec!["src", "main.rs"]
    );
    assert!(Sandbox::components(".").unwrap().is_empty());
    assert_eq!(
        Sandbox::components("src\\deep\\x.rs").unwrap().join("/"),
        "src/deep/x.rs"
    );
    assert!(matches!(Sandbox::components(""), Err(Denied::Empty)));
    assert!(matches!(Sandbox::components("   "), Err(Denied::Empty)));
    assert!(matches!(
        Sandbox::components("../x"),
        Err(Denied::Outside(_))
    ));
    assert!(matches!(
        Sandbox::components("src/../../x"),
        Err(Denied::Outside(_))
    ));
    // a backslash is a separator on every platform, so this is `..` on Linux too
    assert!(matches!(
        Sandbox::components("..\\..\\etc\\passwd"),
        Err(Denied::Outside(_))
    ));
    assert!(matches!(
        Sandbox::components("/etc/passwd"),
        Err(Denied::Absolute(_))
    ));
    assert!(matches!(
        Sandbox::components("C:\\x"),
        Err(Denied::Absolute(_))
    ));
    assert!(matches!(
        Sandbox::components("c:x"),
        Err(Denied::Absolute(_))
    ));
    assert!(matches!(
        Sandbox::components("\\\\server\\share"),
        Err(Denied::Absolute(_))
    ));
    assert!(matches!(
        Sandbox::components("~/x"),
        Err(Denied::BadName(..))
    ));
    assert!(matches!(
        Sandbox::components("notes.md:hidden"),
        Err(Denied::BadName(..))
    ));
    assert!(matches!(
        Sandbox::components("src/main.rs."),
        Err(Denied::BadName(..))
    ));
    assert!(matches!(
        Sandbox::components("src /main.rs"),
        Err(Denied::BadName(..))
    ));
    // surrounding whitespace is the model's, not the path's
    assert_eq!(
        Sandbox::components("  src/main.rs \n").unwrap().join("/"),
        "src/main.rs"
    );
    assert!(matches!(
        Sandbox::components("NUL"),
        Err(Denied::BadName(..))
    ));
    assert!(matches!(
        Sandbox::components("src/con.txt"),
        Err(Denied::BadName(..))
    ));
    assert!(matches!(
        Sandbox::components("lpt1"),
        Err(Denied::BadName(..))
    ));
    // `console.rs` is not `CON`
    assert!(Sandbox::components("src/console.rs").is_ok());
}

#[test]
fn the_deny_rules() {
    let (_d, sb) = tree();
    assert!(matches!(denied(sb.read(".env")), Denied::Secret(_)));
    assert!(matches!(denied(sb.read("src/id_rsa")), Denied::Secret(_)));
    assert!(matches!(
        denied(sb.read(".git/config")),
        Denied::DenyList(_)
    ));
    assert!(matches!(
        denied(sb.read(".GIT/config")),
        Denied::DenyList(_)
    ));
    assert!(matches!(
        denied(sb.read("sub/.git/config")),
        Denied::DenyList(_)
    ));
    assert!(matches!(denied(sb.list(".git")), Denied::DenyList(_)));
    assert!(matches!(
        denied(sb.read(".github/workflows/ci.yml")),
        Denied::DenyList(_)
    ));
    assert!(matches!(
        denied(sb.write(".github/workflows/new.yml", "x", Eol::Lf, false, None)),
        Denied::DenyList(_)
    ));
    // the same rule with no `!` escape hatch: the model has none
    assert!(matches!(
        denied(sb.write(".env", "x", Eol::Lf, false, None)),
        Denied::Secret(_)
    ));
}

#[test]
fn reads_and_what_it_refuses() {
    let (_d, sb) = tree();
    let f = sb.read("src/main.rs").unwrap();
    assert_eq!(f.rel, "src/main.rs");
    assert_eq!(f.text, "fn main() {}\n");
    assert_eq!(f.eol, Eol::Lf);
    assert!(!f.bom);
    assert_eq!(f.hash, hash(b"fn main() {}\n"));
    // CRLF and BOM are taken off for the model and remembered
    let w = sb.read("src/win.rs").unwrap();
    assert_eq!(w.text, "a\nb\n");
    assert_eq!(w.eol, Eol::CrLf);
    assert!(w.bom);
    assert!(matches!(denied(sb.read("nope.rs")), Denied::NotFound(_)));
    assert!(matches!(denied(sb.read("src")), Denied::IsDir(_)));
    assert!(matches!(denied(sb.read(".")), Denied::IsDir(_)));
    assert!(matches!(denied(sb.read("logo.png")), Denied::Binary(_)));
    assert!(matches!(denied(sb.read("big.txt")), Denied::TooLarge(..)));
    // a file in the middle of the path: nothing exists past it
    assert!(matches!(
        denied(sb.read("README.md/x")),
        Denied::NotFound(_)
    ));
}

#[test]
fn lists_folders_first_and_skips_what_the_panel_skips() {
    let (_d, sb) = tree();
    let names: Vec<String> = sb
        .list(".")
        .unwrap()
        .into_iter()
        .map(|e| {
            if e.dir {
                format!("{}/", e.name)
            } else {
                e.name
            }
        })
        .collect();
    assert_eq!(
        names,
        vec![
            ".github/",
            "src/",
            ".env",
            "README.md",
            "big.txt",
            "logo.png"
        ]
    );
    assert!(sb
        .list("src/deep")
        .unwrap()
        .iter()
        .any(|e| e.name == "x.rs"));
    assert!(matches!(denied(sb.list("nope")), Denied::NotFound(_)));
    assert!(matches!(denied(sb.list("README.md")), Denied::NotDir(_)));
}

#[test]
fn writes_atomically_and_keeps_the_file_convention() {
    let (d, sb) = tree();
    let f = sb.read("src/main.rs").unwrap();
    let h = sb
        .write(
            "src/main.rs",
            "fn main() { hi() }\n",
            f.eol,
            f.bom,
            Some(f.hash),
        )
        .unwrap();
    assert_eq!(
        fs::read_to_string(d.path().join("src/main.rs")).unwrap(),
        "fn main() { hi() }\n"
    );
    assert_eq!(h, hash(b"fn main() { hi() }\n"));
    assert!(!d.path().join("src/main.rs.moon-tmp").exists());
    // CRLF and the BOM come back on disk
    let w = sb.read("src/win.rs").unwrap();
    sb.write("src/win.rs", "a\nb\nc\n", w.eol, w.bom, Some(w.hash))
        .unwrap();
    assert_eq!(
        fs::read(d.path().join("src/win.rs")).unwrap(),
        b"\xEF\xBB\xBFa\r\nb\r\nc\r\n"
    );
    // a new file, three folders deep: the folders are made
    sb.write("a/b/c/new.rs", "new\n", Eol::Lf, false, None)
        .unwrap();
    assert_eq!(
        fs::read_to_string(d.path().join("a/b/c/new.rs")).unwrap(),
        "new\n"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let p = d.path().join("src/exec.sh");
        fs::write(&p, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
        let f = sb.read("src/exec.sh").unwrap();
        sb.write(
            "src/exec.sh",
            "#!/bin/sh\necho hi\n",
            f.eol,
            f.bom,
            Some(f.hash),
        )
        .unwrap();
        assert_eq!(
            fs::metadata(&p).unwrap().permissions().mode() & 0o777,
            0o755
        );
    }
}

#[test]
fn writes_it_refuses() {
    let (d, sb) = tree();
    let f = sb.read("src/main.rs").unwrap();
    // never read: no hash to check against
    assert!(matches!(
        denied(sb.write("src/main.rs", "x", Eol::Lf, false, None)),
        Denied::NotRead(_)
    ));
    // changed behind our back
    fs::write(d.path().join("src/main.rs"), "changed\n").unwrap();
    assert!(matches!(
        denied(sb.write("src/main.rs", "x", Eol::Lf, false, Some(f.hash))),
        Denied::Stale(_)
    ));
    // a "new" file that already exists, and an existing one that vanished
    assert!(matches!(
        denied(sb.write("README.md", "x", Eol::Lf, false, None)),
        Denied::NotRead(_)
    ));
    assert!(matches!(
        denied(sb.write("gone.rs", "x", Eol::Lf, false, Some(1))),
        Denied::NotFound(_)
    ));
    assert!(matches!(
        denied(sb.write("src", "x", Eol::Lf, false, Some(1))),
        Denied::IsDir(_)
    ));
    // over the limit, whatever the file was
    let big = "x".repeat(300);
    assert!(matches!(
        denied(sb.write("new.txt", &big, Eol::Lf, false, None)),
        Denied::TooLarge(..)
    ));
    assert!(!d.path().join("new.txt").exists());
}

#[cfg(unix)]
#[test]
fn symlinks_out_are_outside_and_nothing_is_written_through_one() {
    use std::os::unix::fs::symlink;
    let (d, sb) = tree();
    let outside = tempfile::tempdir().unwrap();
    fs::write(outside.path().join("outside.txt"), "s\n").unwrap();
    symlink(
        outside.path().join("outside.txt"),
        d.path().join("link.txt"),
    )
    .unwrap();
    symlink(outside.path(), d.path().join("linkdir")).unwrap();
    symlink(d.path().join("src"), d.path().join("srclink")).unwrap();
    // a link that leaves the tree is outside, file or folder
    assert!(matches!(denied(sb.read("link.txt")), Denied::Outside(_)));
    assert!(matches!(
        denied(sb.read("linkdir/outside.txt")),
        Denied::Outside(_)
    ));
    assert!(matches!(denied(sb.list("linkdir")), Denied::Outside(_)));
    assert!(matches!(
        denied(sb.write("linkdir/new.txt", "x", Eol::Lf, false, None)),
        Denied::Outside(_)
    ));
    // one that stays inside can be read through, never written through
    let f = sb.read("srclink/main.rs").unwrap();
    assert_eq!(f.text, "fn main() {}\n");
    assert!(matches!(
        denied(sb.write("srclink/main.rs", "x", Eol::Lf, false, Some(f.hash))),
        Denied::Symlink(_)
    ));
    assert!(matches!(
        denied(sb.write("srclink/new.rs", "x", Eol::Lf, false, None)),
        Denied::Symlink(_)
    ));
    // a FIFO is not a regular file: reading it would block for ever
    let fifo = d.path().join("pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .is_ok_and(|s| s.success());
    if made {
        assert!(matches!(denied(sb.read("pipe")), Denied::NotAFile(_)));
    }
}

#[test]
fn an_absolute_path_inside_the_root_is_taken_as_relative() {
    let (d, sb) = tree();
    // as the user pastes it, with the root as given and as canonical
    for root in [d.path().to_path_buf(), sb.root().to_path_buf()] {
        let abs = root.join("src/main.rs");
        let abs = abs.to_str().unwrap();
        assert_eq!(sb.relative(abs).unwrap(), "src/main.rs");
        assert_eq!(sb.read(abs).unwrap().text, "fn main() {}\n");
        let names: Vec<String> = sb
            .list(root.to_str().unwrap())
            .unwrap()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(names.contains(&"src".to_string()));
    }
    // still every other check: deny list, `..` out of the root, a sibling
    // folder that only shares the prefix
    let git = d.path().join(".git/config");
    assert!(matches!(
        denied(sb.read(git.to_str().unwrap())),
        Denied::DenyList(_)
    ));
    let up = format!("{}/src/../../x", d.path().display());
    assert!(matches!(denied(sb.read(&up)), Denied::Outside(_)));
    let sibling = format!("{}-other/x.rs", d.path().display());
    assert!(matches!(denied(sb.read(&sibling)), Denied::Absolute(_)));
    assert!(matches!(
        denied(sb.read("/etc/passwd")),
        Denied::Absolute(_)
    ));
}

#[test]
fn find_gives_the_files_a_short_path_may_mean() {
    let (d, sb) = tree();
    fs::create_dir_all(d.path().join("front/src/deep")).unwrap();
    fs::write(d.path().join("front/src/deep/x.rs"), "y\n").unwrap();
    fs::create_dir_all(d.path().join("target/src/deep")).unwrap();
    fs::write(d.path().join("target/src/deep/x.rs"), "z\n").unwrap();
    assert_eq!(
        sb.find("deep/x.rs"),
        vec!["front/src/deep/x.rs", "src/deep/x.rs"]
    );
    // `target/` skipped, and no match on half a name
    assert_eq!(sb.find("src/deep/x.rs"), vec!["front/src/deep/x.rs"]);
    assert!(sb.find("p/x.rs").is_empty());
    // nothing on the deny list
    assert!(sb.find("config").is_empty());
}

#[test]
fn the_root_is_canonical() {
    // on macOS the temp dir is under /private; a file found through the
    // un-canonical root must still count as inside
    let (d, sb) = tree();
    assert_eq!(sb.root(), fs::canonicalize(d.path()).unwrap());
    let loc = sb.resolve("src/main.rs").unwrap();
    assert!(loc.exists);
    assert!(!loc.via_symlink);
    assert!(loc.full.starts_with(sb.root()));
    let new = sb.resolve("src/later.rs").unwrap();
    assert!(!new.exists);
    assert_eq!(new.rel, "src/later.rs");
    assert!(Path::new(&new.full).starts_with(sb.root()));
}

#[test]
fn encode_and_decode_are_inverse() {
    for (bytes, eol, bom) in [
        (b"a\nb\n".to_vec(), Eol::Lf, false),
        (b"a\r\nb\r\n".to_vec(), Eol::CrLf, false),
        (b"\xEF\xBB\xBFa\r\nb".to_vec(), Eol::CrLf, true),
        (b"".to_vec(), Eol::Lf, false),
    ] {
        let f = decode("x".into(), &bytes);
        assert_eq!(f.eol, eol);
        assert_eq!(f.bom, bom);
        assert_eq!(encode(&f.text, f.eol, f.bom), bytes);
    }
    assert_eq!(Eol::detect("no newline"), Eol::Lf);
    assert_eq!(Eol::detect("\r\n"), Eol::CrLf);
}
