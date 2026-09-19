//! Generic list with fuzzy filtering (nucleo, Helix's matcher), drawn by the
//! bottom panel.

use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

#[derive(Debug, Clone, PartialEq)]
pub struct PickerItem {
    /// What is returned on selection (qualified model id, session id…).
    pub id: String,
    /// Text the filter runs on.
    pub key: String,
    pub label: String,
    pub detail: String,
    /// Marked with `●`: the currently active element.
    pub active: bool,
    /// Dimmed: cannot be chosen (provider down…).
    pub dim: bool,
    /// Section it belongs to, if the list is grouped.
    pub group: Option<String>,
}

/// Section header of a grouped list: `title ─────── ● info`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerGroup {
    pub title: String,
    /// Text on the right: the provider's host, its error, a count…
    pub info: String,
    /// `Some(true)` → `●` in `moon`, `Some(false)` → dimmed `✗`, `None` → nothing.
    pub mark: Option<bool>,
}

/// A row of the list as it is drawn.
pub enum Row<'a> {
    Header(&'a PickerGroup),
    /// Element, whether it is under the cursor, and its place in the filtered
    /// list (what `jump` and the cursor work on, not the number drawn).
    Item(&'a PickerItem, bool, usize),
    Blank,
}

pub struct Picker {
    pub title: String,
    /// Text to the right of the title (counts…).
    pub title_info: String,
    /// Second row of the panel while nothing is typed: what this list is for.
    pub hint: String,
    /// Footer shortcuts: (key, action).
    pub keys: Vec<(&'static str, &'static str)>,
    pub empty_text: String,
    /// Sections, in order. Empty: flat list.
    pub groups: Vec<PickerGroup>,
    items: Vec<PickerItem>,
    filtered: Vec<usize>,
    pub query: String,
    pub selected: usize,
    /// Row number being typed, while a longer one is still possible.
    pub pending: Option<usize>,
    matcher: Matcher,
}

impl Picker {
    pub fn new(title: &str, items: Vec<PickerItem>, query: &str) -> Self {
        let mut p = Self {
            title: title.to_string(),
            title_info: String::new(),
            hint: "type to filter".to_string(),
            keys: vec![
                ("↑↓", "move"),
                ("number", "jump"),
                ("enter", "select"),
                ("esc", "close"),
            ],
            empty_text: "nothing to show".to_string(),
            groups: Vec::new(),
            items,
            filtered: Vec::new(),
            query: query.to_string(),
            selected: 0,
            pending: None,
            matcher: Matcher::new(Config::DEFAULT),
        };
        p.refilter();
        p
    }

    pub fn set_items(&mut self, items: Vec<PickerItem>) {
        let keep = self.current().map(|i| i.key.clone());
        self.items = items;
        self.refilter();
        if let Some(k) = keep {
            if let Some(pos) = self.filtered.iter().position(|&i| self.items[i].key == k) {
                self.selected = pos;
            }
        }
    }

    /// Replaces elements, sections and texts with those of `fresh`, keeping
    /// the filter and the selection.
    pub fn update_from(&mut self, fresh: Picker) {
        self.groups = fresh.groups;
        self.title_info = fresh.title_info;
        self.hint = fresh.hint;
        self.empty_text = fresh.empty_text;
        self.set_items(fresh.items);
    }

    pub fn refilter(&mut self) {
        let q = self.query.trim();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let pat = Pattern::parse(q, CaseMatching::Ignore, Normalization::Smart);
            let mut buf = Vec::new();
            let mut scored: Vec<(usize, u32)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, it)| {
                    pat.score(Utf32Str::new(&it.key, &mut buf), &mut self.matcher)
                        .map(|s| (i, s))
                })
                .collect();
            scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
            self.filtered = scored.into_iter().map(|(i, _)| i).collect();
        }
        if self.selected >= self.filtered.len() {
            self.selected = self.filtered.len().saturating_sub(1);
        }
        // with no explicit selection, the active one gets the focus first
        if self.selected == 0 {
            if let Some(pos) = self.filtered.iter().position(|&i| self.items[i].active) {
                if q.is_empty() {
                    self.selected = pos;
                }
            }
        }
    }

    pub fn push(&mut self, c: char) {
        self.query.push(c);
        self.selected = 0;
        self.refilter();
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected = 0;
        self.refilter();
    }

    pub fn up(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + self.filtered.len() - 1) % self.filtered.len();
        }
    }

    pub fn down(&mut self) {
        if !self.filtered.is_empty() {
            self.selected = (self.selected + 1) % self.filtered.len();
        }
    }

    /// Moves the cursor `delta` positions without wrapping around (for the wheel).
    pub fn move_by(&mut self, delta: i32) {
        if self.filtered.is_empty() {
            return;
        }
        let max = self.filtered.len() as i32 - 1;
        self.selected = (self.selected as i32 + delta).clamp(0, max) as usize;
    }

    pub fn len(&self) -> usize {
        self.filtered.len()
    }

    pub fn is_empty(&self) -> bool {
        self.filtered.is_empty()
    }

    pub fn current(&self) -> Option<&PickerItem> {
        self.filtered.get(self.selected).map(|&i| &self.items[i])
    }

    /// Filtered elements in order, with whether they are selected and where
    /// they sit in the filtered list.
    pub fn visible(&self) -> impl Iterator<Item = (&PickerItem, bool, usize)> {
        self.filtered
            .iter()
            .enumerate()
            .map(move |(pos, &i)| (&self.items[i], pos == self.selected, pos))
    }

    /// Puts the cursor on the `n`-th element as it is drawn, sections and all
    /// (1-based, the number the row carries). `false` if there is no such row.
    pub fn jump(&mut self, n: usize) -> bool {
        let pos = self
            .rows()
            .iter()
            .filter_map(|r| match r {
                Row::Item(_, _, pos) => Some(*pos),
                _ => None,
            })
            .nth(n.saturating_sub(1));
        match pos {
            Some(pos) => {
                self.selected = pos;
                true
            }
            None => false,
        }
    }

    /// One digit of a row number. The cursor goes to that row right away; the
    /// answer says whether it can only be that one, because no longer number
    /// reaches the list, and there is nothing left to wait for. A digit that
    /// does not extend what was being typed starts a number of its own.
    pub fn number(&mut self, c: char) -> bool {
        let d = c.to_digit(10).unwrap_or(0) as usize;
        let total = self.len();
        let n = match self.pending.map(|p| p * 10 + d) {
            Some(n) if n <= total => n,
            _ => d,
        };
        self.pending = None;
        if n == 0 || !self.jump(n) {
            return false;
        }
        if n * 10 > total {
            return true;
        }
        self.pending = Some(n);
        false
    }

    /// Rows to draw. With sections: header and elements of each one, with a
    /// blank row between sections; while filtering, sections without matches
    /// disappear (without a filter they are shown, even if empty, so that a
    /// provider that is down explains why there is nothing of its own).
    pub fn rows(&self) -> Vec<Row<'_>> {
        if self.groups.is_empty() {
            return self
                .visible()
                .map(|(i, s, pos)| Row::Item(i, s, pos))
                .collect();
        }
        let filtering = !self.query.trim().is_empty();
        let mut out = Vec::new();
        for g in &self.groups {
            let items: Vec<Row> = self
                .visible()
                .filter(|(i, ..)| i.group.as_deref() == Some(g.title.as_str()))
                .map(|(i, s, pos)| Row::Item(i, s, pos))
                .collect();
            if items.is_empty() && filtering {
                continue;
            }
            if !out.is_empty() {
                out.push(Row::Blank);
            }
            out.push(Row::Header(g));
            out.extend(items);
        }
        out
    }

    /// Index, within `rows()`, of the row under the cursor.
    pub fn selected_row(&self) -> Option<usize> {
        self.rows()
            .iter()
            .position(|r| matches!(r, Row::Item(_, true, _)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(key: &str, active: bool) -> PickerItem {
        PickerItem {
            id: key.into(),
            key: key.into(),
            label: key.into(),
            detail: String::new(),
            active,
            dim: false,
            group: None,
        }
    }

    #[test]
    fn filtra_y_mueve() {
        let mut p = Picker::new(
            "Model",
            vec![
                item("ollama/llama3.1:8b", false),
                item("ollama/qwen2.5-coder:14b", true),
                item("lmstudio/qwen3-8b", false),
            ],
            "",
        );
        assert_eq!(p.len(), 3);
        assert_eq!(p.current().unwrap().key, "ollama/qwen2.5-coder:14b"); // the active one starts selected
        p.push('q');
        p.push('w');
        assert_eq!(p.len(), 2);
        assert!(p.visible().all(|(i, ..)| i.key.contains("qwen")));
        p.down();
        p.down();
        assert_eq!(p.selected, 0);
        p.backspace();
        p.backspace();
        p.push('z');
        assert!(p.is_empty());
        assert!(p.current().is_none());
    }

    #[test]
    fn filas_por_seccion() {
        let mut a = item("ollama/llama3.1:8b", false);
        a.group = Some("ollama".into());
        let mut b = item("ollama/qwen2.5-coder:14b", true);
        b.group = Some("ollama".into());
        let mut c = item("ollama/qwen2.5-coder:14b", true);
        c.group = Some("Recent".into());
        let mut p = Picker::new("Model", vec![c, a, b], "");
        p.groups = vec![
            PickerGroup {
                title: "Recent".into(),
                info: "1 model".into(),
                mark: None,
            },
            PickerGroup {
                title: "ollama".into(),
                info: "localhost:11434".into(),
                mark: Some(true),
            },
            PickerGroup {
                title: "openai".into(),
                info: "no API key".into(),
                mark: Some(false),
            },
        ];
        let kinds: Vec<char> = p
            .rows()
            .iter()
            .map(|r| match r {
                Row::Header(_) => 'H',
                Row::Item(_, true, _) => 'S',
                Row::Item(_, false, _) => 'i',
                Row::Blank => ' ',
            })
            .collect();
        // the active one starts selected: its copy in Recent; openai comes out empty
        assert_eq!(kinds.iter().collect::<String>(), "HS Hii H");
        assert_eq!(p.selected_row(), Some(1));
        p.down();
        p.down();
        assert_eq!(p.selected_row(), Some(5));
        // while filtering, sections without matches disappear ("ll" would match
        // "ollama" in all of them; "3.1" only llama3.1)
        p.push('3');
        p.push('.');
        p.push('1');
        let kinds: Vec<char> = p
            .rows()
            .iter()
            .map(|r| match r {
                Row::Header(_) => 'H',
                Row::Item(..) => 'i',
                Row::Blank => ' ',
            })
            .collect();
        assert_eq!(kinds.iter().collect::<String>(), "Hi");
    }
}
