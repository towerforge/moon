//! The globs of the deny list: `.git/**`, `secrets/*.toml`, `**/id_rsa`.
//! `*` and `?` inside one segment, `**` for any number of segments, always
//! anchored at the project root and never case-sensitive.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Glob {
    segs: Vec<Seg>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Seg {
    /// `**`: zero or more segments.
    Any,
    Lit(String),
}

impl Glob {
    pub fn new(pattern: &str) -> Self {
        let segs = pattern
            .replace('\\', "/")
            .trim_matches('/')
            .split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .map(|s| {
                if s == "**" {
                    Seg::Any
                } else {
                    Seg::Lit(s.to_lowercase())
                }
            })
            .collect();
        Self { segs }
    }

    /// `rel` is `/`-separated and relative to the root.
    pub fn matches(&self, rel: &str) -> bool {
        let path: Vec<String> = rel
            .split('/')
            .filter(|s| !s.is_empty() && *s != ".")
            .map(str::to_lowercase)
            .collect();
        let path: Vec<&str> = path.iter().map(String::as_str).collect();
        match_segs(&self.segs, &path)
    }
}

fn match_segs(pat: &[Seg], path: &[&str]) -> bool {
    match pat.split_first() {
        None => path.is_empty(),
        Some((Seg::Any, rest)) => (0..=path.len()).any(|i| match_segs(rest, &path[i..])),
        Some((Seg::Lit(p), rest)) => match path.split_first() {
            Some((s, tail)) => match_seg(p, s) && match_segs(rest, tail),
            None => false,
        },
    }
}

/// `*` any run of characters, `?` one, the rest literal.
fn match_seg(pat: &str, s: &str) -> bool {
    let p: Vec<char> = pat.chars().collect();
    let t: Vec<char> = s.chars().collect();
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => (0..=t.len()).any(|i| go(rest, &t[i..])),
            Some(('?', rest)) => !t.is_empty() && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    go(&p, &t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn segments_and_stars() {
        let g = Glob::new(".github/workflows/**");
        assert!(g.matches(".github/workflows/ci.yml"));
        assert!(g.matches(".github/workflows/a/b.yml"));
        // `**` also matches nothing: the folder itself is covered
        assert!(g.matches(".github/workflows"));
        assert!(!g.matches(".github/dependabot.yml"));
        assert!(Glob::new("**/id_rsa").matches("home/x/id_rsa"));
        assert!(Glob::new("**/id_rsa").matches("id_rsa"));
        assert!(Glob::new("secrets/*.toml").matches("secrets/prod.toml"));
        assert!(!Glob::new("secrets/*.toml").matches("secrets/sub/prod.toml"));
        assert!(Glob::new("*.env").matches("prod.env"));
        assert!(Glob::new("a?c").matches("abc"));
        assert!(!Glob::new("a?c").matches("ac"));
    }

    #[test]
    fn case_and_separators_do_not_matter() {
        assert!(Glob::new(".GIT/**").matches(".git/config"));
        assert!(Glob::new(".git\\**").matches(".git/hooks/pre-commit"));
        assert!(Glob::new("/docs/*.md").matches("./docs/x.md"));
    }
}
