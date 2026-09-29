//! Agents as files: one TOML per agent under the agents folder next to the
//! configuration, the file name being the agent's name. `default.toml` is
//! the agent every conversation starts with, `reviewer.toml` the sample
//! `moon config init` writes, and any other is the user's own. A file that
//! does not parse is reported and skipped — the rest still load, and a
//! broken file is never rewritten. The file's `[permissions]` GRANT: they
//! are the agent's whole truth, so the folder carries the same trust as
//! the configuration — only ever the user's own, never a project's.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use moon_core::config::ids::SUBFOLDERS_OLD;
use moon_core::{write_text, Permission, ToolsConfig};
use serde::Deserialize;

use super::{reviewer, AgentDef, DEFAULT_AGENT, DEFAULT_STEPS};
use crate::tools::catalog::{Category, Policy, CATALOG};

/// A `<name>.toml` under the agents folder, as written. Everything is
/// optional: a file with nothing in it is an agent with nothing on. An
/// `inherit` left over from the two-layer days parses and is ignored.
#[derive(Debug, Clone, Default, Deserialize, PartialEq)]
#[serde(default)]
pub struct AgentFile {
    /// One line for the picker.
    pub description: String,
    /// Its own step limit; none is `DEFAULT_STEPS`.
    pub max_steps: Option<usize>,
    /// Its own prompt; none means moon's own — the editor's when it may
    /// change files, the reader's otherwise.
    pub prompt: Option<String>,
    /// By the ids the panel shows; one the catalogue does not have is
    /// dropped, as everywhere else. What is not listed is off.
    pub permissions: BTreeMap<String, Permission>,
}

/// The comment on top of every agent file moon writes or rewrites.
pub const AGENT_FILE_HEADER: &str = "\
# An agent for /agent: a prompt and its permissions, whole. Every capability
# moon knows is listed in its group, set to \"off\" (not offered), \"ask\"
# (shown, and waits for your ok) or \"allow\" (runs on its own); a line left
# out is off. Choosing the agent is choosing these permissions. Without a
# prompt the agent gets moon's own. moon rewrites this file whole from /agent
# and when a new version adds capabilities: comments of your own do not
# survive it.

";

/// The name of an agent's file.
pub fn file_name(name: &str) -> String {
    format!("{name}.toml")
}

/// What `tools.toml` held before agents carried their permissions: read
/// once, when `default.toml` is first written, and never again.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
struct LegacyTools {
    max_steps: Option<usize>,
    permissions: BTreeMap<String, Permission>,
}

impl AgentFile {
    /// What `ctrl+a` in the picker starts from: reading on, nothing else.
    pub fn template() -> Self {
        AgentFile {
            description: "say in one line what this agent is for".into(),
            max_steps: Some(DEFAULT_STEPS),
            prompt: Some(
                "Say here how the agent works: what it does, what it must not do, and \
                 how it reports."
                    .into(),
            ),
            permissions: BTreeMap::from([("read files".to_string(), Permission::Allow)]),
        }
    }

    /// The file a definition would be written as.
    pub fn from_def(def: &AgentDef) -> Self {
        AgentFile {
            description: def.description.clone(),
            max_steps: def.max_steps,
            prompt: def.prompt.clone(),
            permissions: def.policy.pairs(),
        }
    }

    /// The two files `moon config init` writes, by name: `default` with
    /// reading on, and the `reviewer`.
    pub fn factory() -> Vec<(&'static str, AgentFile)> {
        vec![
            (
                DEFAULT_AGENT,
                AgentFile::from_def(&AgentDef::factory_default()),
            ),
            ("reviewer", AgentFile::from_def(&reviewer())),
        ]
    }

    /// One file of the folder, parsed; the error is one line.
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        toml::from_str(&text).map_err(|e| one_line(&e.to_string()))
    }

    /// The policy the file describes: unknown ids dropped, the old name of
    /// `commands in subfolders` read as the new one, and the rule between
    /// reading and writing applied.
    pub fn policy(&self) -> Policy {
        Policy::from_pairs(self.permissions.clone())
    }

    /// The file as moon writes it: the header, the fields, the prompt as a
    /// multiline string, and every entry of the catalogue in its group with
    /// its permission, `off` included, and what it does as a comment, so
    /// the file shows everything there is to set.
    pub fn to_toml(&self) -> String {
        let policy = self.policy();
        let mut out = String::from(AGENT_FILE_HEADER);
        out.push_str(&format!(
            "description = \"{}\"\n",
            escape(&self.description).replace('\n', "\\n"),
        ));
        if let Some(n) = self.max_steps {
            out.push_str(&format!("max_steps   = {n}\n"));
        }
        match &self.prompt {
            Some(p) => out.push_str(&format!("\nprompt = \"\"\"\n{}\"\"\"\n", escape(p))),
            None => out.push_str(
                "\n# no prompt: moon's own, the editor's when it may write files, the \
                 reader's otherwise\n",
            ),
        }
        out.push_str("\n[permissions]\n");
        let key_w = CATALOG.iter().map(|e| e.id.len() + 2).max().unwrap_or(10);
        for (n, cat) in Category::ALL.into_iter().enumerate() {
            if n > 0 {
                out.push('\n');
            }
            out.push_str(&format!("# {} — {}\n", cat.title(), cat.about()));
            let mut section = None;
            for e in CATALOG.iter().filter(|e| e.category == cat) {
                if let Some(s) = e.section.filter(|_| e.section != section) {
                    section = e.section;
                    out.push_str(&format!("# {}\n", s.title()));
                }
                let key = format!("\"{}\"", e.id);
                let value = format!("\"{}\"", policy.get(e.id).as_str());
                out.push_str(&format!("{key:<key_w$} = {value:<7}  # {}\n", e.help));
            }
        }
        out
    }

    /// What a rewrite would change in the listing: capabilities of the
    /// catalogue the file does not name, and names it has that the
    /// catalogue no longer knows. `None` when it lists exactly what there is.
    pub fn stale(&self) -> Option<(usize, usize)> {
        let new = CATALOG
            .iter()
            .filter(|e| !self.permissions.contains_key(e.id))
            .filter(|e| {
                !(e.id == moon_core::config::ids::SUBFOLDERS
                    && self.permissions.contains_key(SUBFOLDERS_OLD))
            })
            .count();
        let gone = self
            .permissions
            .keys()
            .filter(|k| !CATALOG.iter().any(|e| e.id == k.as_str()))
            .count();
        (new + gone > 0).then_some((new, gone))
    }

    /// The definition the file describes, named after the file.
    pub fn def(self, name: &str) -> Result<AgentDef, String> {
        if name.is_empty() {
            return Err("the file needs a name".into());
        }
        Ok(AgentDef {
            name: name.into(),
            description: self.description,
            prompt: self.prompt,
            policy: Policy::from_pairs(self.permissions),
            max_steps: self.max_steps,
        })
    }
}

/// `default.toml` made when it is not there, so every conversation has an
/// agent to start with. What it starts from, in order: `tools.toml` next
/// to the folder, if one is left from before agents carried their
/// permissions; the old `[tools]` keys of the configuration; or reading on
/// and nothing else. Says what it started from (`with reading on`, …), or
/// nothing when the file was there.
pub fn ensure_default(dir: &Path, legacy: &ToolsConfig) -> Result<Option<String>, String> {
    let path = dir.join(file_name(DEFAULT_AGENT));
    if path.exists() {
        return Ok(None);
    }
    let mut file = AgentFile::from_def(&AgentDef::factory_default());
    let old = dir.parent().map(|d| d.join("tools.toml"));
    let from = match old.filter(|p| p.exists()) {
        Some(old) => match std::fs::read_to_string(&old)
            .map_err(|e| e.to_string())
            .and_then(|t| toml::from_str::<LegacyTools>(&t).map_err(|e| one_line(&e.to_string())))
        {
            Ok(t) => {
                file.permissions = t.permissions;
                file.max_steps = Some(t.max_steps.unwrap_or(DEFAULT_STEPS));
                "with what tools.toml had; that file is no longer read and can go".to_string()
            }
            Err(e) => format!("with reading on; tools.toml could not be read ({e})"),
        },
        None => {
            let p = legacy.startup_permissions();
            if p.is_empty() {
                "with reading on".to_string()
            } else {
                file.permissions = p;
                "with what the old [tools] keys of config.toml said; they are no longer read"
                    .to_string()
            }
        }
    };
    write_text(&path, &file.to_toml()).map_err(|e| e.to_string())?;
    Ok(Some(from))
}

/// Every file of the folder brought up to the catalogue: one that names
/// fewer capabilities than there are, or names one there is not, is
/// rewritten whole — with what it set kept — and said. A file that does
/// not parse is left alone: `defs_from_dir` reports it.
pub fn sync_dir(dir: &Path) -> Vec<String> {
    let mut said = Vec::new();
    for path in toml_files(dir) {
        let Ok(file) = AgentFile::load(&path) else {
            continue;
        };
        let Some((new, gone)) = file.stale() else {
            continue;
        };
        let shown = shown_name(&path);
        match write_text(&path, &file.to_toml()) {
            Ok(()) => {
                let mut what = Vec::new();
                if new > 0 {
                    what.push(format!("{new} new"));
                }
                if gone > 0 {
                    what.push(format!("{gone} gone"));
                }
                said.push(format!(
                    "agents/{shown} brought up to the catalogue · {}",
                    what.join(", ")
                ));
            }
            Err(e) => said.push(format!("agents/{shown} could not be rewritten: {e}")),
        }
    }
    said
}

/// Every agent defined in `dir`, `default` first and the rest by file
/// name, and one line per file that could not be taken. A folder that
/// does not exist is simply empty.
pub fn defs_from_dir(dir: &Path) -> (Vec<AgentDef>, Vec<String>) {
    let mut defs = Vec::new();
    let mut errors = Vec::new();
    for path in toml_files(dir) {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        match AgentFile::load(&path).and_then(|f| f.def(&name)) {
            Ok(d) => defs.push(d),
            Err(e) => errors.push(format!("{}: {e}", shown_name(&path))),
        }
    }
    defs.sort_by_key(|d| (d.name != DEFAULT_AGENT, d.name.clone()));
    (defs, errors)
}

/// The `.toml` files of the folder, sorted; none for a folder that is not
/// there or cannot be read.
fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "toml"))
        .collect();
    files.sort();
    files
}

fn shown_name(path: &Path) -> String {
    path.file_name()
        .and_then(|s| s.to_str())
        .unwrap_or_default()
        .to_string()
}

/// A TOML error spans lines; a notice has one.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// What a TOML string cannot carry as it is; newlines stay literal in the
/// multiline prompt.
fn escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use moon_core::config::ids::{EDIT_FILES, READ_FILES, SUBFOLDERS};

    fn write(dir: &Path, name: &str, text: &str) {
        std::fs::write(dir.join(name), text).unwrap();
    }

    const COMMITTER: &str = r##"
description = "stages and commits what you approve"
inherit     = false
max_steps   = 6
prompt      = "# Committing\n\nYou prepare commits."

[permissions]
"read files" = "allow"
"git diff"   = "allow"
"git add"    = "ask"
"git commit" = "ask"
"rm -rf"     = "allow"
"##;

    #[test]
    fn a_folder_of_files_loads_what_parses_and_says_what_does_not() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "committer.toml", COMMITTER);
        write(
            dir.path(),
            "broken.toml",
            "description = \"x\"\nprompt = 3\n",
        );
        // default is a file like any other, and comes first whatever its name sorts as
        write(
            dir.path(),
            "default.toml",
            "[permissions]\n\"ls\" = \"allow\"\n",
        );
        write(dir.path(), "notes.txt", "not an agent");
        let (defs, errors) = defs_from_dir(dir.path());
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["default", "committer"], "{errors:?}");
        let d = &defs[1];
        assert_eq!(d.max_steps, Some(6));
        // its permissions are the file's, whole: the unknown id dropped,
        // the rest as written, the unlisted off — and the old `inherit`
        // line parses and is ignored
        assert_eq!(d.policy.get("read files"), Permission::Allow);
        assert_eq!(d.policy.get("git add"), Permission::Ask);
        assert_eq!(d.policy.get("git commit"), Permission::Ask);
        assert_eq!(d.policy.get(EDIT_FILES), Permission::Off);
        assert_eq!(d.policy.get("rm -rf"), Permission::Off);
        // a file with only permissions: no description, moon's own prompt
        assert_eq!(defs[0].description, "");
        assert_eq!(defs[0].prompt, None);
        assert_eq!(defs[0].agent().name, "reader");
        // the one that could not be taken, named
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].starts_with("broken.toml:"), "{errors:?}");
        // a folder that is not there is just empty
        let (none, errs) = defs_from_dir(&dir.path().join("missing"));
        assert!(none.is_empty() && errs.is_empty());
    }

    #[test]
    fn what_moon_writes_lists_everything_and_reads_back_as_it_was() {
        // the template, and a file with everything the escaping must survive
        let tricky = AgentFile {
            description: "a \"quoted\" one\nwith a second line".into(),
            max_steps: Some(6),
            prompt: Some(
                "Say \"hi\", use C:\\x, even \"\"\" fences.\n\nAnd a blank line.\n".into(),
            ),
            permissions: BTreeMap::from([
                ("git diff".to_string(), Permission::Allow),
                ("read files".to_string(), Permission::Ask),
                ("git add".to_string(), Permission::Off),
            ]),
        };
        for f in [AgentFile::template(), tricky] {
            let text = f.to_toml();
            assert!(text.starts_with("# An agent for /agent"), "{text}");
            let back: AgentFile = toml::from_str(&text).expect(&text);
            assert_eq!(back.description, f.description);
            assert_eq!(back.prompt, f.prompt);
            assert_eq!(back.max_steps, f.max_steps);
            // every entry, in its group, off included: the file shows what there is
            for e in CATALOG {
                assert!(back.permissions.contains_key(e.id), "{}", e.id);
                assert!(text.contains(&format!("\"{}\"", e.id)), "{}", e.id);
            }
            for c in Category::ALL {
                assert!(
                    text.contains(&format!("# {} — ", c.title())),
                    "{}",
                    c.title()
                );
            }
            assert_eq!(back.policy(), f.policy(), "{text}");
            assert!(back.stale().is_none());
        }
        // the help rides along, and Stack titles each language
        let text = AgentFile::template().to_toml();
        let ls = text.lines().find(|l| l.starts_with("\"ls\"")).unwrap();
        assert!(
            ls.contains("= \"off\"") && ls.contains("# list a folder"),
            "{ls}"
        );
        let rust = text.find("\n# Rust\n").expect("rust title");
        assert!(text.find("\n# Stack — ").unwrap() < rust, "{text}");
        // no prompt is said, not left blank
        let bare = AgentFile::from_def(&AgentDef::factory_default()).to_toml();
        assert!(bare.contains("# no prompt: moon's own"), "{bare}");
        assert!(!bare.contains("\nprompt = "), "{bare}");
        let back: AgentFile = toml::from_str(&bare).unwrap();
        assert_eq!(back.prompt, None);
        assert_eq!(back.policy().get(READ_FILES), Permission::Allow);
    }

    #[test]
    fn a_short_file_is_stale_and_a_rewrite_keeps_what_it_set() {
        let dir = tempfile::tempdir().unwrap();
        // the old name of a capability counts as gone, and is written under the new one
        write(
            dir.path(),
            "short.toml",
            "description = \"x\"\n\n[permissions]\n\"git diff\" = \"ask\"\n\"run inside subfolders\" = \"allow\"\n\"rm -rf\" = \"allow\"\n",
        );
        write(dir.path(), "broken.toml", "prompt = 3\n");
        let f = AgentFile::load(&dir.path().join("short.toml")).unwrap();
        let (new, gone) = f.stale().expect("stale");
        assert_eq!(
            new,
            CATALOG.len() - 2,
            "git diff and, under its old name, subfolders are the two named"
        );
        assert_eq!(gone, 2, "the old name and the unknown one");
        let said = sync_dir(dir.path());
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(
            said[0].starts_with("agents/short.toml brought up to the catalogue"),
            "{said:?}"
        );
        assert!(
            said[0].contains("new") && said[0].contains("2 gone"),
            "{said:?}"
        );
        let f = AgentFile::load(&dir.path().join("short.toml")).unwrap();
        assert!(f.stale().is_none());
        assert_eq!(f.policy().get("git diff"), Permission::Ask);
        assert!(f.policy().subfolders(), "kept under its new name");
        assert!(!f.permissions.contains_key("rm -rf"));
        assert!(f.permissions.contains_key(SUBFOLDERS));
        assert_eq!(f.description, "x");
        // the broken one is not touched, and a second pass changes nothing
        assert_eq!(
            std::fs::read_to_string(dir.path().join("broken.toml")).unwrap(),
            "prompt = 3\n"
        );
        assert!(sync_dir(dir.path()).is_empty());
    }

    #[test]
    fn default_is_made_from_what_there_was() {
        let conf = tempfile::tempdir().unwrap();
        let dir = conf.path().join("agents");
        let none = ToolsConfig::default();
        // nothing before: reading on, and said once
        let said = ensure_default(&dir, &none).unwrap().expect("written");
        assert!(said.contains("with reading on"), "{said}");
        let f = AgentFile::load(&dir.join("default.toml")).unwrap();
        assert_eq!(f.policy().get(READ_FILES), Permission::Allow);
        assert_eq!(f.policy().entries().len(), 1);
        assert_eq!(f.max_steps, Some(DEFAULT_STEPS));
        assert!(f.stale().is_none());
        // there already: nothing said, nothing touched
        assert_eq!(ensure_default(&dir, &none).unwrap(), None);
        // an old tools.toml next to the folder is what default starts from
        std::fs::remove_file(dir.join("default.toml")).unwrap();
        std::fs::write(
            conf.path().join("tools.toml"),
            "max_steps = 5\n[permissions]\n\"read files\" = \"allow\"\n\"git diff\" = \"ask\"\n",
        )
        .unwrap();
        let said = ensure_default(&dir, &none).unwrap().unwrap();
        assert!(said.contains("tools.toml"), "{said}");
        let f = AgentFile::load(&dir.join("default.toml")).unwrap();
        assert_eq!(f.max_steps, Some(5));
        assert_eq!(f.policy().get("git diff"), Permission::Ask);
        // without one, the old [tools] keys of the configuration
        std::fs::remove_file(dir.join("default.toml")).unwrap();
        std::fs::remove_file(conf.path().join("tools.toml")).unwrap();
        let legacy = ToolsConfig {
            enabled: true,
            edit: true,
            create: false,
            ..ToolsConfig::default()
        };
        let said = ensure_default(&dir, &legacy).unwrap().unwrap();
        assert!(said.contains("[tools]"), "{said}");
        let f = AgentFile::load(&dir.join("default.toml")).unwrap();
        assert_eq!(f.policy().get(EDIT_FILES), Permission::Ask);
        assert!(!f.policy().creates());
    }

    #[test]
    fn everything_is_optional_and_the_factory_files_are_whole() {
        let f: AgentFile = toml::from_str("").unwrap();
        assert_eq!(f, AgentFile::default());
        assert!(f.clone().def("").is_err());
        let d = f.def("mine").unwrap();
        assert!(d.policy.is_empty() && d.prompt.is_none());
        let names: Vec<&str> = AgentFile::factory().iter().map(|(n, _)| *n).collect();
        assert_eq!(names, vec!["default", "reviewer"]);
        for (name, f) in AgentFile::factory() {
            assert!(!f.description.is_empty(), "{name}");
            assert_eq!(f.max_steps, Some(DEFAULT_STEPS), "{name}");
            assert!(f.policy().reads(), "{name}");
        }
        let (_, reviewer) = &AgentFile::factory()[1];
        assert!(reviewer.prompt.as_deref().unwrap().contains("# Reviewing"));
        assert_eq!(file_name("x"), "x.toml");
    }
}
