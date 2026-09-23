//! A line diff between what a file is and what it would become, for the
//! approval panel and the `+3 −1` of the step line. Computed here so the
//! harness can count; painting it is the interface's job.

use similar::{ChangeTag, TextDiff};

/// Context lines around each change.
const CONTEXT: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Diff {
    pub lines: Vec<DiffLine>,
    pub added: usize,
    pub removed: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiffKind {
    Context,
    Added,
    Removed,
    /// Lines left out between two hunks.
    Gap,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffLine {
    pub kind: DiffKind,
    /// Line number in the file as it is, and as it would be (1-based).
    pub old: Option<usize>,
    pub new: Option<usize>,
    /// Without its line break.
    pub text: String,
}

pub fn diff(before: &str, after: &str) -> Diff {
    let d = TextDiff::from_lines(before, after);
    let mut out = Diff::default();
    for (i, group) in d.grouped_ops(CONTEXT).iter().enumerate() {
        if i > 0 {
            out.lines.push(DiffLine {
                kind: DiffKind::Gap,
                old: None,
                new: None,
                text: String::new(),
            });
        }
        for op in group {
            for change in d.iter_changes(op) {
                let kind = match change.tag() {
                    ChangeTag::Equal => DiffKind::Context,
                    ChangeTag::Delete => DiffKind::Removed,
                    ChangeTag::Insert => DiffKind::Added,
                };
                match kind {
                    DiffKind::Added => out.added += 1,
                    DiffKind::Removed => out.removed += 1,
                    _ => {}
                }
                out.lines.push(DiffLine {
                    kind,
                    old: change.old_index().map(|i| i + 1),
                    new: change.new_index().map(|i| i + 1),
                    text: change.value().trim_end_matches(['\n', '\r']).to_string(),
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_context() {
        let before = (1..=20).map(|i| format!("l{i}\n")).collect::<String>();
        let after = before.replace("l5\n", "L5\nL5b\n").replace("l18\n", "");
        let d = diff(&before, &after);
        assert_eq!((d.added, d.removed), (2, 2));
        // two hunks far apart get a gap between them
        assert_eq!(
            d.lines.iter().filter(|l| l.kind == DiffKind::Gap).count(),
            1
        );
        let removed: Vec<&str> = d
            .lines
            .iter()
            .filter(|l| l.kind == DiffKind::Removed)
            .map(|l| l.text.as_str())
            .collect();
        assert_eq!(removed, vec!["l5", "l18"]);
        let first = &d.lines[0];
        assert_eq!(first.kind, DiffKind::Context);
        assert_eq!((first.old, first.new), (Some(2), Some(2)));
        // a new file is all additions, with no old numbers
        let n = diff("", "a\nb\n");
        assert_eq!((n.added, n.removed), (2, 0));
        assert!(n.lines.iter().all(|l| l.old.is_none()));
        assert!(diff("same\n", "same\n").lines.is_empty());
    }
}
