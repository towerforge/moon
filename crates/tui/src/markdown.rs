//! Markdown → ratatui lines with the Moon palette. Code highlighting does not
//! use syntect's colors: keywords in `moon-soft`, comments in `ink-muted`,
//! strings in `moon`, the rest in `ink`.

use std::str::FromStr;

use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SynColor, ScopeSelectors, StyleModifier, Theme as SynTheme, ThemeItem,
};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

use crate::theme::Theme;
use crate::wrap::{pad_line, truncate, width, wrap_line};

// Sentinel colors: the syntect theme only serves to classify; the real color
// is set by `Theme` when converting.
const SENT_INK: SynColor = SynColor {
    r: 0,
    g: 0,
    b: 0,
    a: 255,
};
const SENT_MUTED: SynColor = SynColor {
    r: 1,
    g: 1,
    b: 1,
    a: 255,
};
const SENT_SOFT: SynColor = SynColor {
    r: 2,
    g: 2,
    b: 2,
    a: 255,
};
const SENT_MOON: SynColor = SynColor {
    r: 3,
    g: 3,
    b: 3,
    a: 255,
};

pub struct Renderer {
    syntaxes: SyntaxSet,
    syn_theme: SynTheme,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        let item = |sel: &str, c: SynColor| ThemeItem {
            scope: ScopeSelectors::from_str(sel).expect("valid scope selector"),
            style: StyleModifier {
                foreground: Some(c),
                background: None,
                font_style: None,
            },
        };
        let mut syn_theme = SynTheme::default();
        syn_theme.settings.foreground = Some(SENT_INK);
        syn_theme.scopes = vec![
            item("comment", SENT_MUTED),
            item("keyword, storage, keyword.operator", SENT_SOFT),
            item("string, constant.numeric, constant.language", SENT_MOON),
        ];
        Self {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            syn_theme,
        }
    }

    pub fn render(&self, md: &str, width: usize, theme: &Theme) -> Vec<Line<'static>> {
        let mut w = Walker::new(self, theme, width.max(10));
        let opts =
            Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
        for ev in Parser::new_ext(md, opts) {
            w.event(ev);
        }
        w.finish()
    }

    fn highlight(&self, lang: &str, code: &str, theme: &Theme) -> Vec<Line<'static>> {
        let bg = theme.raised();
        let syntax = if lang.is_empty() {
            None
        } else {
            self.syntaxes
                .find_syntax_by_token(lang)
                .or_else(|| self.syntaxes.find_syntax_by_extension(lang))
        };
        let Some(syntax) = syntax else {
            return code
                .lines()
                .map(|l| Line::from(Span::styled(l.to_string(), bg)))
                .collect();
        };
        let mut hl = HighlightLines::new(syntax, &self.syn_theme);
        let mut out = Vec::new();
        for line in LinesWithEndings::from(code) {
            let spans = match hl.highlight_line(line, &self.syntaxes) {
                Ok(pieces) => pieces
                    .into_iter()
                    .map(|(st, text)| {
                        let fg = match st.foreground {
                            c if c == SENT_MUTED => theme.ink_muted,
                            c if c == SENT_SOFT => theme.moon_soft,
                            c if c == SENT_MOON => theme.moon,
                            _ => theme.ink,
                        };
                        Span::styled(text.trim_end_matches('\n').to_string(), bg.fg(fg))
                    })
                    .collect(),
                Err(_) => vec![Span::styled(line.trim_end_matches('\n').to_string(), bg)],
            };
            out.push(Line::from(spans));
        }
        out
    }
}

struct TableState {
    rows: Vec<Vec<String>>,
    cur_row: Vec<String>,
    has_header: bool,
}

struct Walker<'a> {
    r: &'a Renderer,
    t: &'a Theme,
    width: usize,
    out: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    styles: Vec<Style>,
    /// Per list level: (next number if ordered, marker width)
    lists: Vec<(Option<u64>, usize)>,
    indent: usize,
    item_marker: Option<String>,
    code: Option<(String, String)>,
    quote: usize,
    table: Option<TableState>,
    link: Option<String>,
}

impl<'a> Walker<'a> {
    fn new(r: &'a Renderer, t: &'a Theme, width: usize) -> Self {
        Self {
            r,
            t,
            width,
            out: Vec::new(),
            cur: Vec::new(),
            styles: vec![t.text()],
            lists: Vec::new(),
            indent: 0,
            item_marker: None,
            code: None,
            quote: 0,
            table: None,
            link: None,
        }
    }

    fn style(&self) -> Style {
        *self
            .styles
            .last()
            .expect("the base style is always present")
    }

    fn push_style(&mut self, f: impl Fn(Style) -> Style) {
        let s = f(self.style());
        self.styles.push(s);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn text(&mut self, s: &str) {
        if let Some(tbl) = self.table.as_mut() {
            if let Some(cell) = tbl.cur_row.last_mut() {
                cell.push_str(s);
            }
            return;
        }
        if let Some((_, buf)) = self.code.as_mut() {
            buf.push_str(s);
            return;
        }
        let st = self.style();
        match self.cur.last_mut() {
            Some(last) if last.style == st => last.content.to_mut().push_str(s),
            _ => self.cur.push(Span::styled(s.to_string(), st)),
        }
    }

    /// Blank line between blocks (not inside lists).
    fn block_gap(&mut self) {
        if self.lists.is_empty()
            && !self.out.is_empty()
            && self.out.last().is_some_and(|l| l.width() > 0)
        {
            self.out.push(Line::from(""));
        }
    }

    fn flush(&mut self) {
        let spans = std::mem::take(&mut self.cur);
        let marker = self.item_marker.take();
        if spans.is_empty() && marker.is_none() {
            return;
        }
        let quote_prefix = "│ ".repeat(self.quote);
        let rest = " ".repeat(self.indent);
        let first = match &marker {
            Some(m) => format!("{}{}", " ".repeat(self.indent.saturating_sub(width(m))), m),
            None => rest.clone(),
        };
        let avail = self
            .width
            .saturating_sub(self.indent + quote_prefix.len())
            .max(8);
        let line = Line::from(spans);
        let line_style = self.t.line();
        let marker_style = self.t.accent();
        for (i, l) in wrap_line(&line, avail).into_iter().enumerate() {
            let mut spans = Vec::new();
            if self.quote > 0 {
                spans.push(Span::styled(quote_prefix.clone(), line_style));
            }
            let prefix = if i == 0 { first.clone() } else { rest.clone() };
            if !prefix.is_empty() {
                spans.push(Span::styled(prefix, marker_style));
            }
            spans.extend(l.spans);
            self.out.push(Line::from(spans));
        }
    }

    fn event(&mut self, ev: Event<'_>) {
        match ev {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(s) => self.text(&s),
            Event::Code(s) => {
                let st = self.t.raised_soft();
                self.cur.push(Span::styled(s.to_string(), st));
            }
            Event::Html(s) | Event::InlineHtml(s) => self.text(&s),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.block_gap();
                let l = Line::from(Span::styled("─".repeat(self.width), self.t.line()));
                self.out.push(l);
            }
            Event::TaskListMarker(done) => self.text(if done { "☑ " } else { "☐ " }),
            Event::FootnoteReference(s) => self.text(&format!("[^{s}]")),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => {
                if self.lists.is_empty() || self.item_marker.is_none() {
                    self.block_gap();
                }
            }
            Tag::Heading { .. } => {
                self.block_gap();
                let moon = self.t.moon;
                self.push_style(|s| s.fg(moon).add_modifier(Modifier::BOLD));
            }
            Tag::BlockQuote(_) => {
                self.block_gap();
                self.quote += 1;
                let muted = self.t.ink_muted;
                self.push_style(|s| s.fg(muted));
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.block_gap();
                let lang = match kind {
                    CodeBlockKind::Fenced(l) => {
                        l.split_whitespace().next().unwrap_or("").to_string()
                    }
                    CodeBlockKind::Indented => String::new(),
                };
                self.code = Some((lang, String::new()));
            }
            Tag::List(start) => {
                self.flush();
                if self.lists.is_empty() {
                    self.block_gap();
                }
                let marker_w = match start {
                    Some(n) => format!("{n}. ").len().max(3),
                    None => 2,
                };
                self.lists.push((start, marker_w));
                self.indent += marker_w;
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some((Some(n), _)) => {
                        let m = format!("{n}. ");
                        *n += 1;
                        m
                    }
                    _ => "• ".to_string(),
                };
                self.item_marker = Some(marker);
            }
            Tag::Emphasis => self.push_style(|s| s.add_modifier(Modifier::ITALIC)),
            Tag::Strong => self.push_style(|s| s.add_modifier(Modifier::BOLD)),
            Tag::Strikethrough => self.push_style(|s| s.add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                self.link = Some(dest_url.to_string());
                self.push_style(|s| s.add_modifier(Modifier::UNDERLINED));
            }
            Tag::Image { dest_url, .. } => {
                self.link = Some(dest_url.to_string());
                self.text("[imagen] ");
            }
            Tag::Table(_) => {
                self.flush();
                self.block_gap();
                self.table = Some(TableState {
                    rows: Vec::new(),
                    cur_row: Vec::new(),
                    has_header: false,
                });
            }
            Tag::TableHead => {
                if let Some(t) = self.table.as_mut() {
                    t.has_header = true;
                }
            }
            Tag::TableRow => {}
            Tag::TableCell => {
                if let Some(t) = self.table.as_mut() {
                    t.cur_row.push(String::new());
                }
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph | TagEnd::Item => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.pop_style();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
                self.pop_style();
            }
            TagEnd::CodeBlock => {
                if let Some((lang, buf)) = self.code.take() {
                    self.render_code(&lang, &buf);
                }
            }
            TagEnd::List(_) => {
                self.flush();
                if let Some((_, w)) = self.lists.pop() {
                    self.indent = self.indent.saturating_sub(w);
                }
            }
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                if let Some(url) = self.link.take() {
                    let shown: String = self
                        .cur
                        .last()
                        .map(|s| s.content.to_string())
                        .unwrap_or_default();
                    if !shown.contains(&url) {
                        let st = self.t.muted();
                        self.cur.push(Span::styled(format!(" ({url})"), st));
                    }
                }
            }
            TagEnd::Image => {
                if let Some(url) = self.link.take() {
                    let st = self.t.muted();
                    self.cur.push(Span::styled(format!(" ({url})"), st));
                }
            }
            TagEnd::TableHead | TagEnd::TableRow => {
                if let Some(t) = self.table.as_mut() {
                    let row = std::mem::take(&mut t.cur_row);
                    t.rows.push(row);
                }
            }
            TagEnd::TableCell => {}
            TagEnd::Table => {
                if let Some(t) = self.table.take() {
                    self.render_table(t);
                }
            }
            _ => {}
        }
    }

    fn render_code(&mut self, lang: &str, code: &str) {
        let code = code.replace('\t', "    ");
        let code = code.strip_suffix('\n').unwrap_or(&code);
        let bg = self.t.raised();
        let inner = self.width.saturating_sub(2).max(8);
        if !lang.is_empty() {
            let l = Line::from(Span::styled(format!(" {lang}"), self.t.raised_muted()));
            self.out.push(pad_line(l, self.width, bg));
        }
        for line in self.r.highlight(lang, code, self.t) {
            for piece in wrap_line(&line, inner) {
                let mut spans = vec![Span::styled(" ", bg)];
                spans.extend(piece.spans);
                self.out.push(pad_line(Line::from(spans), self.width, bg));
            }
        }
    }

    fn render_table(&mut self, t: TableState) {
        let ncols = t.rows.iter().map(|r| r.len()).max().unwrap_or(0);
        if ncols == 0 {
            return;
        }
        let cap = (self.width.saturating_sub(3 * ncols) / ncols).max(4);
        let mut widths = vec![0usize; ncols];
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                widths[i] = widths[i].max(width(cell.trim()).min(cap));
            }
        }
        let line_style = self.t.line();
        for (ri, row) in t.rows.iter().enumerate() {
            let mut spans = Vec::new();
            for (i, w) in widths.iter().enumerate() {
                let cell = row.get(i).map(|c| c.trim()).unwrap_or("");
                let text = truncate(cell, *w);
                let pad = w.saturating_sub(width(&text));
                let st = if ri == 0 && t.has_header {
                    self.t.bold()
                } else {
                    self.t.text()
                };
                spans.push(Span::styled(format!(" {text}{} ", " ".repeat(pad)), st));
                if i + 1 < ncols {
                    spans.push(Span::styled("│", line_style));
                }
            }
            self.out.push(Line::from(spans));
            if ri == 0 && t.has_header {
                let sep: Vec<String> = widths.iter().map(|w| "─".repeat(w + 2)).collect();
                self.out
                    .push(Line::from(Span::styled(sep.join("┼"), line_style)));
            }
        }
    }

    fn finish(mut self) -> Vec<Line<'static>> {
        self.flush();
        if let Some((lang, buf)) = self.code.take() {
            self.render_code(&lang, &buf);
        }
        while self.out.last().is_some_and(|l| l.width() == 0) {
            self.out.pop();
        }
        self.out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn paragraphs_headings_lists() {
        let r = Renderer::new();
        let t = Theme::moon();
        let md = "# Title\n\nA paragraph with **bold** and `code`.\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n";
        let out = r.render(md, 40, &t);
        let tx = texts(&out);
        assert_eq!(tx[0], "Title");
        assert_eq!(out[0].spans[0].style.fg, Some(t.moon));
        assert_eq!(tx[1], "");
        assert_eq!(tx[2], "A paragraph with bold and code.");
        assert_eq!(tx[4], "• one");
        assert_eq!(tx[5], "• two");
        assert_eq!(tx[6], "  • nested");
        assert_eq!(tx[8], "1. first");
        assert_eq!(tx[9], "2. second");
    }

    #[test]
    fn code_block_with_background_and_full_width() {
        let r = Renderer::new();
        let t = Theme::moon();
        let md = "text\n\n```rust\nfn main() {} // note\n```\n";
        let out = r.render(md, 30, &t);
        let tx = texts(&out);
        assert_eq!(tx[0], "text");
        assert_eq!(tx[2].trim_end(), " rust");
        assert_eq!(out[2].width(), 30);
        assert_eq!(tx[3].trim_end(), " fn main() {} // note");
        assert_eq!(out[3].width(), 30);
        // the `fn` keyword goes in moon-soft and the comment in ink-muted
        let fg: Vec<_> = out[3]
            .spans
            .iter()
            .map(|s| (s.content.to_string(), s.style.fg))
            .collect();
        assert!(fg
            .iter()
            .any(|(c, f)| c.trim() == "fn" && *f == Some(t.moon_soft)));
        assert!(fg
            .iter()
            .any(|(c, f)| c.contains("note") && *f == Some(t.ink_muted)));
        assert!(out[3]
            .spans
            .iter()
            .all(|s| s.style.bg == Some(t.night_raised)));
    }

    #[test]
    fn unclosed_fence_while_streaming() {
        let r = Renderer::new();
        let t = Theme::moon();
        let out = r.render("```py\nprint(1)", 20, &t);
        let tx = texts(&out);
        assert_eq!(tx[0].trim_end(), " py");
        assert_eq!(tx[1].trim_end(), " print(1)");
    }

    #[test]
    fn table_and_quote() {
        let r = Renderer::new();
        let t = Theme::moon();
        let md = "| a | b |\n|---|---|\n| 1 | 22 |\n\n> quote\n";
        let out = r.render(md, 40, &t);
        let tx = texts(&out);
        assert_eq!(tx[0], " a │ b  ");
        assert!(tx[1].starts_with("───┼"));
        assert_eq!(tx[2], " 1 │ 22 ");
        assert_eq!(tx[4], "│ quote");
    }
}
