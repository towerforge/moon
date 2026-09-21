//! `@path` mentions in the text of a message.

/// Mentioned specs (without the `@`), unique and in order of appearance. The
/// usual trailing punctuation is stripped: `@src/a.rs,` or `@b.rs.` at the end
/// of a sentence.
pub fn extract(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for tok in text.split_whitespace() {
        let Some(rest) = tok.strip_prefix('@') else {
            continue;
        };
        let mut rest = rest.trim_end_matches([',', ';', ')', '"', '\'', ':']);
        if let Some(stripped) = rest.strip_suffix('.') {
            if stripped.contains(['.', '/']) {
                rest = stripped;
            }
        }
        // `@@` is not a path: it is the hunk header of a diff
        if rest.is_empty() || rest == "!" || rest.starts_with('@') {
            continue;
        }
        if !out.iter().any(|o| o == rest) {
            out.push(rest.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_mentions() {
        assert_eq!(
            extract("explica @src/a.rs y @b.rs:1-3, gracias. Mira @src/a.rs otra vez"),
            vec!["src/a.rs", "b.rs:1-3"]
        );
        assert_eq!(extract("escribe a a@b.com y @ solo"), Vec::<String>::new());
        assert_eq!(extract("usa @!.env."), vec!["!.env"]);
        assert_eq!(extract("(ver @docs/x.md)"), vec!["docs/x.md"]);
        assert_eq!(
            extract("@@ -127,6 +127,8 @@ fn render()"),
            Vec::<String>::new()
        );
    }
}
