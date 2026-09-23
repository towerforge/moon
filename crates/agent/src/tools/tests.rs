//! The four tools against a real tree, and what they answer the model.

use std::fs;

use serde_json::json;

use super::*;
use crate::sandbox::{encode, Eol};

fn tree() -> (tempfile::TempDir, Sandbox) {
    let dir = tempfile::tempdir().unwrap();
    let r = dir.path();
    fs::create_dir_all(r.join("src")).unwrap();
    fs::write(
        r.join("src/main.rs"),
        "fn main() {\n    hi();\n    hi();\n}\n",
    )
    .unwrap();
    fs::write(r.join("src/win.rs"), "a\r\nb\r\n").unwrap();
    fs::write(r.join("README.md"), "# hi\n").unwrap();
    let sb = Sandbox::new(r, 10_000, &[]).unwrap();
    (dir, sb)
}

#[test]
fn names_specs_and_verbs() {
    for t in Tool::ALL {
        assert_eq!(Tool::from_name(t.name()), Some(t));
        let s = t.spec();
        assert_eq!(s.name, t.name());
        assert!(!s.description.is_empty());
        assert_eq!(s.parameters["type"], "object");
    }
    assert_eq!(Tool::from_name("bash"), None);
    assert!(Tool::EditFile.writes() && Tool::WriteFile.writes());
    assert!(!Tool::ReadFile.writes() && !Tool::ListDir.writes());
    assert_eq!(Tool::EditFile.verb(), "edit");
}

#[test]
fn path_of_names_the_path_relative_even_when_it_was_absolute() {
    let (_d, sb) = tree();
    assert_eq!(path_of(&sb, &json!({"path": "./src//a.rs"})), "src/a.rs");
    let abs = sb.root().join("src/a.rs");
    assert_eq!(
        path_of(&sb, &json!({"path": abs.to_str().unwrap()})),
        "src/a.rs"
    );
    assert_eq!(path_of(&sb, &json!({})), "?");
}

#[test]
fn read_file_not_found_points_to_the_file_it_may_mean() {
    let (d, sb) = tree();
    fs::create_dir_all(d.path().join("frontend/src/pages/[id]")).unwrap();
    fs::write(d.path().join("frontend/src/pages/[id]/index.tsx"), "x\n").unwrap();
    let mut seen = SeenFiles::new();
    let e =
        read_file::run(&sb, &mut seen, &json!({"path": "src/pages/[id]/index.tsx"})).unwrap_err();
    assert!(
        matches!(e, ToolError::Usage(ref m) if m.contains("`frontend/src/pages/[id]/index.tsx`")),
        "{e:?}"
    );
    // with nothing like it, the plain refusal
    let e = read_file::run(&sb, &mut seen, &json!({"path": "nope.rs"})).unwrap_err();
    assert!(matches!(e, ToolError::Denied(Denied::NotFound(_))), "{e:?}");
}

#[test]
fn read_file_gives_the_file_block_and_remembers_it() {
    let (_d, sb) = tree();
    let mut seen = SeenFiles::new();
    let out = read_file::run(&sb, &mut seen, &json!({"path": "src/main.rs"})).unwrap();
    assert!(
        out.starts_with("<file path=\"src/main.rs\">\n```rs\nfn main() {"),
        "{out}"
    );
    assert!(seen.contains_key("src/main.rs"));
    let out = read_file::run(
        &sb,
        &mut seen,
        &json!({"path": "src/main.rs", "range": "2-3"}),
    )
    .unwrap();
    assert!(out.contains("lines=\"2-3\""));
    assert!(out.contains("    hi();\n    hi();\n```"));
    assert!(matches!(
        read_file::run(
            &sb,
            &mut seen,
            &json!({"path": "src/main.rs", "range": "9-"})
        ),
        Err(ToolError::Usage(_))
    ));
    assert!(matches!(
        read_file::run(
            &sb,
            &mut seen,
            &json!({"path": "src/main.rs", "range": "x"})
        ),
        Err(ToolError::Usage(_))
    ));
    assert!(matches!(
        read_file::run(&sb, &mut seen, &json!({"nope": 1})),
        Err(ToolError::Usage(_))
    ));
    assert!(matches!(
        read_file::run(&sb, &mut seen, &json!({"path": "../x"})),
        Err(ToolError::Denied(Denied::Outside(_)))
    ));
    // CRLF never reaches the model
    let out = read_file::run(&sb, &mut seen, &json!({"path": "src/win.rs"})).unwrap();
    assert!(!out.contains('\r'));
}

#[test]
fn list_dir_lists() {
    let (_d, sb) = tree();
    let out = list_dir::run(&sb, &json!({})).unwrap();
    assert_eq!(out, "src/\nREADME.md (5 bytes)");
    let out = list_dir::run(&sb, &json!({"path": "src"})).unwrap();
    assert!(out.starts_with("main.rs ("));
    assert!(matches!(
        list_dir::run(&sb, &json!({"path": "nope"})),
        Err(ToolError::Denied(Denied::NotFound(_)))
    ));
    fs::create_dir(_d.path().join("empty")).unwrap();
    assert_eq!(
        list_dir::run(&sb, &json!({"path": "empty"})).unwrap(),
        "`empty` is empty"
    );
}

#[test]
fn edit_file_needs_a_read_and_a_unique_match() {
    let (d, sb) = tree();
    let mut seen = SeenFiles::new();
    let call =
        json!({"path": "src/main.rs", "old_string": "    hi();\n", "new_string": "    bye();\n"});
    assert!(matches!(
        edit_file::prepare(&sb, &seen, &call),
        Err(ToolError::Denied(Denied::NotRead(_)))
    ));
    read_file::run(&sb, &mut seen, &json!({"path": "src/main.rs"})).unwrap();
    // twice in the file: ambiguous without replace_all
    assert!(matches!(
        edit_file::prepare(&sb, &seen, &call),
        Err(ToolError::Usage(ref m)) if m.contains("occurs 2 times")
    ));
    let all = json!({"path": "src/main.rs", "old_string": "    hi();\n", "new_string": "    bye();\n", "replace_all": true});
    let e = edit_file::prepare(&sb, &seen, &all).unwrap();
    assert_eq!(e.after, "fn main() {\n    bye();\n    bye();\n}\n");
    assert_eq!((e.diff.added, e.diff.removed), (2, 2));
    assert_eq!(e.counts(), "+2 −2");
    assert!(e.expect.is_some());
    // nothing on disk until it is applied
    assert!(fs::read_to_string(d.path().join("src/main.rs"))
        .unwrap()
        .contains("hi();"));
    let h = e.apply(&sb).unwrap();
    assert_eq!(
        fs::read_to_string(d.path().join("src/main.rs")).unwrap(),
        "fn main() {\n    bye();\n    bye();\n}\n"
    );
    assert_ne!(h, e.expect.unwrap());
    // the file changed since the read: stale until read again
    assert!(matches!(
        edit_file::prepare(&sb, &seen, &all),
        Err(ToolError::Denied(Denied::Stale(_)))
    ));
    read_file::run(&sb, &mut seen, &json!({"path": "src/main.rs"})).unwrap();
    assert!(matches!(
        edit_file::prepare(&sb, &seen, &json!({"path": "src/main.rs", "old_string": "nope", "new_string": "x"})),
        Err(ToolError::Usage(ref m)) if m.contains("not found")
    ));
    assert!(matches!(
        edit_file::prepare(
            &sb,
            &seen,
            &json!({"path": "src/main.rs", "old_string": "", "new_string": "x"})
        ),
        Err(ToolError::Usage(_))
    ));
    assert!(matches!(
        edit_file::prepare(&sb, &seen, &json!({"path": "src/main.rs", "old_string": "fn main", "new_string": "fn main"})),
        Err(ToolError::Usage(ref m)) if m.contains("unchanged")
    ));
    // CRLF in the arguments matches a file that was read as LF, and the
    // file keeps its own line endings when written
    read_file::run(&sb, &mut seen, &json!({"path": "src/win.rs"})).unwrap();
    let w = edit_file::prepare(
        &sb,
        &seen,
        &json!({"path": "src/win.rs", "old_string": "a\r\nb\r\n", "new_string": "a\nc\n"}),
    )
    .unwrap();
    assert_eq!(w.eol, Eol::CrLf);
    w.apply(&sb).unwrap();
    assert_eq!(
        fs::read(d.path().join("src/win.rs")).unwrap(),
        b"a\r\nc\r\n"
    );
}

#[test]
fn write_file_creates_or_replaces() {
    let (d, sb) = tree();
    let mut seen = SeenFiles::new();
    let new = write_file::prepare(
        &sb,
        &seen,
        &json!({"path": "docs/a/b.md", "content": "# new\n"}),
    )
    .unwrap();
    assert_eq!(new.expect, None);
    assert_eq!(new.before, "");
    assert_eq!((new.diff.added, new.diff.removed), (1, 0));
    // a new file has no convention of its own: it takes the platform's, so
    // on Windows the bytes on disk are CRLF
    assert_eq!(new.eol, Eol::platform());
    new.apply(&sb).unwrap();
    assert_eq!(
        fs::read(d.path().join("docs/a/b.md")).unwrap(),
        encode("# new\n", Eol::platform(), false)
    );
    // an existing file must have been read
    let over = json!({"path": "README.md", "content": "# bye\n"});
    assert!(matches!(
        write_file::prepare(&sb, &seen, &over),
        Err(ToolError::Denied(Denied::NotRead(_)))
    ));
    read_file::run(&sb, &mut seen, &json!({"path": "README.md"})).unwrap();
    let e = write_file::prepare(&sb, &seen, &over).unwrap();
    assert_eq!(e.before, "# hi\n");
    assert_eq!((e.diff.added, e.diff.removed), (1, 1));
    assert!(matches!(
        write_file::prepare(
            &sb,
            &seen,
            &json!({"path": "README.md", "content": "# hi\n"})
        ),
        Err(ToolError::Usage(_))
    ));
    assert!(matches!(
        write_file::prepare(&sb, &seen, &json!({"path": ".env", "content": "x"})),
        Err(ToolError::Denied(Denied::Secret(_)))
    ));
}

#[test]
fn an_empty_file_is_filled_with_an_empty_old_string() {
    let (d, sb) = tree();
    let mut seen = SeenFiles::new();
    fs::write(d.path().join("empty.txt"), "").unwrap();
    read_file::run(&sb, &mut seen, &json!({"path": "empty.txt"})).unwrap();
    // what a model means by editing an empty file: put this in it
    let fill = edit_file::prepare(
        &sb,
        &seen,
        &json!({"path": "empty.txt", "old_string": "", "new_string": "one\ntwo\n"}),
    )
    .unwrap();
    assert_eq!(fill.after, "one\ntwo\n");
    assert_eq!((fill.diff.added, fill.diff.removed), (2, 0));
    assert!(matches!(
        edit_file::prepare(
            &sb,
            &seen,
            &json!({"path": "empty.txt", "old_string": "", "new_string": "  "})
        ),
        Err(ToolError::Usage(_))
    ));
    // on a file with content the empty old_string is still refused, and
    // the message says what to do instead
    read_file::run(&sb, &mut seen, &json!({"path": "README.md"})).unwrap();
    assert!(matches!(
        edit_file::prepare(&sb, &seen, &json!({"path": "README.md", "old_string": "", "new_string": "x"})),
        Err(ToolError::Usage(ref m)) if m.contains("has content")
    ));
}
