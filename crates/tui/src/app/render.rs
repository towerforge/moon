//! Rendering the conversation: cached lines, scrolling, mouse selection, the welcome block.

use super::*;

impl App {
    // ----- rendering the conversation ---------------------------------------

    /// Screen → document: (absolute row of the conversation, column).
    pub(super) fn doc_pos(&self, x: u16, y: u16) -> Option<(usize, usize)> {
        let conv = self.conv_area?;
        let y = y.clamp(conv.y, conv.bottom().saturating_sub(1).max(conv.y));
        let x = x.clamp(conv.x, conv.right().saturating_sub(1).max(conv.x));
        Some((
            self.scroll_offset + (y - conv.y) as usize,
            (x - conv.x) as usize,
        ))
    }

    pub(super) fn mouse_down(&mut self, x: u16, y: u16) {
        self.selection = None;
        let pos = ratatui::layout::Position { x, y };
        if self.jump_rect.is_some_and(|r| r.contains(pos)) {
            self.follow = true;
            return;
        }
        // in the box the drag selects what is being written, to copy it
        if let Some(r) = self.input_area.filter(|r| r.contains(pos)) {
            if self.panel.is_none() {
                self.input.select_from(r.width, x - r.x, y - r.y);
                self.input_drag = true;
            }
            return;
        }
        self.input.clear_selection();
        if self.conv_area.is_some_and(|r| r.contains(pos)) {
            if let Some(p) = self.doc_pos(x, y) {
                self.selection = Some(Selection {
                    anchor: p,
                    head: p,
                    dragging: true,
                });
            }
        }
    }

    pub(super) fn mouse_drag(&mut self, x: u16, y: u16) {
        if self.input_drag {
            if let Some(r) = self.input_area {
                let x = x.clamp(r.x, r.right().saturating_sub(1).max(r.x));
                let y = y.clamp(r.y, r.bottom().saturating_sub(1).max(r.y));
                self.input.select_to(r.width, x - r.x, y - r.y);
            }
            return;
        }
        let Some(sel) = self.selection.as_mut() else {
            return;
        };
        if !sel.dragging {
            return;
        }
        let Some(conv) = self.conv_area else { return };
        // below the area: up to the end of the last visible row
        let head = if y >= conv.bottom() {
            (
                self.scroll_offset + conv.height.saturating_sub(1) as usize,
                conv.width.saturating_sub(1) as usize,
            )
        } else if y < conv.y {
            (self.scroll_offset, 0)
        } else {
            (
                self.scroll_offset + (y - conv.y) as usize,
                (x.clamp(conv.x, conv.right().saturating_sub(1).max(conv.x)) - conv.x) as usize,
            )
        };
        sel.head = head;
    }

    pub(super) fn mouse_up(&mut self, x: u16, y: u16) {
        // what was selected in the box goes to the clipboard on letting go,
        // like a selection over the conversation
        if self.input_drag {
            self.mouse_drag(x, y);
            self.input_drag = false;
            let text = self.input.selection_text();
            if text.is_empty() {
                self.input.clear_selection();
                return;
            }
            self.copy_selection(&text);
            return;
        }
        let Some(sel) = self.selection.as_mut() else {
            return;
        };
        if !sel.dragging {
            return;
        }
        sel.dragging = false;
        if y >= self.conv_area.map_or(0, |c| c.y) {
            self.mouse_drag(x, y);
            if let Some(sel) = self.selection.as_mut() {
                sel.dragging = false;
            }
        }
        if self.selection.is_some_and(|s| s.anchor == s.head) {
            self.selection = None;
            return;
        }
        let text = self.selection_text();
        if text.trim().is_empty() {
            self.selection = None;
            return;
        }
        self.copy_selection(&text);
    }

    fn copy_selection(&mut self, text: &str) {
        let n = text.chars().count();
        match crate::clipboard::copy(text) {
            Ok(via) => self.notify(format!("selection copied · {n} chars ({via})")),
            Err(e) => self.notify(format!("could not copy: {e}")),
        }
    }

    /// Text of the current selection: full rows in the middle, cut by columns
    /// at the ends, without trailing spaces.
    pub fn selection_text(&mut self) -> String {
        let Some(sel) = self.selection else {
            return String::new();
        };
        let Some(conv) = self.conv_area else {
            return String::new();
        };
        let ((r1, c1), (r2, c2)) = sel.bounds();
        let lines = self.doc_lines(conv.width, r1, r2 - r1 + 1);
        let mut out = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let row = r1 + i;
            let from = if row == r1 { c1 } else { 0 };
            let to = if row == r2 { c2 + 1 } else { usize::MAX };
            out.push(
                slice_columns(&line.to_string(), from, to)
                    .trim_end()
                    .to_string(),
            );
        }
        out.join("\n")
    }

    /// Document lines starting at row `from`, without touching the scroll.
    pub(super) fn doc_lines(
        &mut self,
        width: u16,
        from: usize,
        count: usize,
    ) -> Vec<Line<'static>> {
        self.ensure_cache(width);
        let welcome = self.welcome_lines();
        let mut out = Vec::with_capacity(count);
        let mut pos = 0usize;
        let mut take = |lines: &[Line<'static>], out: &mut Vec<Line<'static>>| {
            for l in lines {
                if out.len() >= count {
                    return;
                }
                if pos >= from {
                    out.push(l.clone());
                }
                pos += 1;
            }
        };
        take(&welcome, &mut out);
        let blank = [Line::from("")];
        // each request opens a turn with a full-width divider in `night-line`
        let rule = [Line::from(Span::styled(
            "─".repeat(width as usize),
            self.theme.line(),
        ))];
        for i in self.view_from..self.items.len() {
            take(&blank, &mut out);
            if is_request(&self.items[i]) {
                take(&rule, &mut out);
            }
            if let Some(r) = &self.cache[i] {
                take(&r.lines, &mut out);
            }
        }
        out
    }

    /// Conversation lines left below the view.
    pub fn lines_below(&self) -> usize {
        self.total_lines
            .saturating_sub(self.scroll_offset + self.view_height)
    }

    pub fn scroll_by(&mut self, delta: i32) {
        let max_off = self.total_lines.saturating_sub(self.view_height);
        let cur = if self.follow {
            max_off
        } else {
            self.scroll_offset
        };
        let next = (cur as i64 + delta as i64).clamp(0, max_off as i64) as usize;
        self.scroll_offset = next;
        self.follow = next >= max_off;
    }

    pub(super) fn ensure_cache(&mut self, width: u16) {
        if self.cache.len() != self.items.len() {
            self.cache.resize_with(self.items.len(), || None);
        }
        for i in self.view_from..self.items.len() {
            let key = item_key(&self.items[i]);
            let ok = self.cache[i]
                .as_ref()
                .is_some_and(|r| r.width == width && r.key == key);
            if !ok {
                let lines = item_lines(&self.items[i], &self.md, &self.theme, width as usize);
                self.cache[i] = Some(Rendered { width, key, lines });
            }
        }
    }

    pub fn welcome_lines(&self) -> Vec<Line<'static>> {
        let t = &self.theme;
        let cur_prov = self
            .current
            .as_ref()
            .and_then(|c| self.providers.iter().find(|p| p.id == c.provider));
        let model_line: Vec<Span<'static>> = match (&self.current, cur_prov) {
            (Some(c), Some(p)) => match &p.health {
                Some(Err(e)) => vec![Span::styled(
                    format!(
                        "✗ {} is not responding at {}: {}",
                        c.provider, p.base_url, e
                    ),
                    t.text(),
                )],
                _ => {
                    let ctx = self
                        .ctx_window()
                        .map(|n| format!(" (ctx {})", fmt_k(n)))
                        .unwrap_or_default();
                    vec![Span::styled(
                        format!("{}{ctx} · {} · {}", c.model, c.provider, p.base_url),
                        t.muted(),
                    )]
                }
            },
            (Some(c), None) => vec![Span::styled(
                format!("✗ provider {} unavailable", c.provider),
                t.text(),
            )],
            (None, _) if self.loading => vec![Span::styled("checking providers…", t.muted())],
            (None, _) => vec![Span::styled("no model · /model to pick one", t.muted())],
        };
        let name_line = vec![
            Span::styled("moon", t.accent_bold()),
            Span::styled(format!(" v{}", self.version), t.muted()),
        ];
        let update_line = self.update_available.as_ref().map(|v| {
            vec![
                Span::styled(format!("↑ v{v} available"), t.accent()),
                Span::styled(" · run `moon update`", t.muted()),
            ]
        });
        let cwd_line = vec![Span::styled(self.cwd.clone(), t.muted())];
        let config_line = self.default_config.then(|| {
            vec![Span::styled(
                "default configuration · `moon config init` writes the file",
                t.muted(),
            )]
        });
        let with_logo = |rows: Vec<String>, right: Vec<Vec<Span<'static>>>, indent: usize| {
            let mut lines: Vec<Line<'static>> = Vec::new();
            let mut right = right.into_iter();
            let pad = " ".repeat(LOGO_PAD);
            for row in rows {
                let mut v = vec![
                    Span::styled(format!("{pad}{row}"), t.accent()),
                    Span::raw("   "),
                ];
                v.extend(right.next().unwrap_or_default());
                lines.push(Line::from(v));
            }
            for spans in right {
                let mut v = vec![Span::raw(" ".repeat(LOGO_PAD + indent + 3))];
                v.extend(spans);
                lines.push(Line::from(v));
            }
            lines
        };
        let mut right = vec![name_line];
        right.extend(update_line);
        right.push(model_line);
        right.push(cwd_line);
        right.extend(config_line);
        // a blank row over the logo: flush against the top edge it felt cramped
        let mut lines = vec![Line::from("")];
        lines.extend(with_logo(logo_rows(), right, LOGO_COLS));
        lines
    }

    /// Visible lines of the conversation for a given area. Updates the scroll
    /// and the auto-follow.
    pub fn visible_lines(&mut self, width: u16, height: u16) -> Vec<Line<'static>> {
        self.ensure_cache(width);
        let welcome = self.welcome_lines();
        let h = height as usize;
        let mut total = welcome.len();
        for i in self.view_from..self.items.len() {
            total += 1
                + usize::from(is_request(&self.items[i]))
                + self.cache[i].as_ref().map_or(0, |r| r.lines.len());
        }
        let max_off = total.saturating_sub(h);
        let offset = if self.follow {
            max_off
        } else {
            self.scroll_offset.min(max_off)
        };
        if offset >= max_off {
            self.follow = true;
        }
        self.scroll_offset = offset;
        self.total_lines = total;
        self.view_height = h;

        let mut out: Vec<Line<'static>> = Vec::with_capacity(h);
        let mut pos = 0usize;
        let mut take = |lines: &[Line<'static>], out: &mut Vec<Line<'static>>| {
            for l in lines {
                if out.len() >= h {
                    return;
                }
                if pos >= offset {
                    out.push(l.clone());
                }
                pos += 1;
            }
        };
        take(&welcome, &mut out);
        let blank = [Line::from("")];
        // each request opens a turn with a full-width divider in `night-line`
        let rule = [Line::from(Span::styled(
            "─".repeat(width as usize),
            self.theme.line(),
        ))];
        for i in self.view_from..self.items.len() {
            take(&blank, &mut out);
            if is_request(&self.items[i]) {
                take(&rule, &mut out);
            }
            if let Some(r) = &self.cache[i] {
                take(&r.lines, &mut out);
            }
        }
        out
    }
}

/// Substring between two screen columns (end exclusive), honoring
/// double-width characters.
pub(super) fn slice_columns(text: &str, from: usize, to: usize) -> String {
    use unicode_width::UnicodeWidthChar;
    let mut col = 0;
    let mut out = String::new();
    for ch in text.chars() {
        let w = UnicodeWidthChar::width(ch).unwrap_or(0);
        if col >= to {
            break;
        }
        if col >= from {
            out.push(ch);
        }
        col += w;
    }
    out
}

/// Prefix of every line of a request: a bar in `moon`.
pub(super) const USER_BAR: &str = "▌ ";

/// A user message: opens a turn in the conversation.
pub(super) fn is_request(item: &Item) -> bool {
    matches!(item, Item::Message(m) if m.role == Role::User)
}

pub(super) fn item_key(item: &Item) -> usize {
    match item {
        Item::Message(m) => {
            m.content.len()
                + m.thinking.as_ref().map_or(0, |t| t.len())
                + usize::from(m.partial)
                + usize::from(m.usage.is_some())
        }
        Item::Error(s) | Item::Info(s) => s.len(),
    }
}

pub(super) fn item_lines(
    item: &Item,
    md: &Renderer,
    t: &Theme,
    width: usize,
) -> Vec<Line<'static>> {
    let width = width.max(10);
    match item {
        Item::Message(m) => match m.role {
            Role::User => {
                // `▌` bar in `moon` along the whole turn, attachments included
                let mut out = Vec::new();
                for l in m.content.lines() {
                    let line = Line::from(Span::styled(l.to_string(), t.muted()));
                    for w in wrap_line(&line, width - 2) {
                        let mut spans = vec![Span::styled(USER_BAR, t.accent())];
                        spans.extend(w.spans);
                        out.push(Line::from(spans));
                    }
                }
                if out.is_empty() {
                    out.push(Line::from(Span::styled(USER_BAR, t.accent())));
                }
                for a in &m.attachments {
                    out.push(Line::from(vec![
                        Span::styled(USER_BAR, t.accent()),
                        Span::styled(
                            format!(
                                "▸ @{} · {} tok{}",
                                a.label(),
                                fmt_k(a.tokens),
                                if a.truncated { " · truncated" } else { "" }
                            ),
                            t.muted(),
                        ),
                    ]));
                }
                out
            }
            Role::Assistant => {
                let mut out = Vec::new();
                if let Some(th) = &m.thinking {
                    out.push(Line::from(Span::styled(
                        format!("▸ hidden reasoning ({} chars)", th.chars().count()),
                        t.muted(),
                    )));
                }
                out.extend(md.render(&m.content, width, t));
                if m.partial {
                    out.push(Line::from(Span::styled("(reply cancelled)", t.muted())));
                }
                out
            }
            Role::System | Role::Tool => {
                let line = Line::from(Span::styled(
                    format!("{}: {}", m.role.as_str(), m.content),
                    t.muted(),
                ));
                wrap_line(&line, width)
            }
        },
        Item::Error(e) => {
            let line = Line::from(Span::styled(format!("✗ {e}"), t.error()));
            wrap_line(&line, width)
                .into_iter()
                .map(|l| pad_line(l, width, t.error()))
                .collect()
        }
        Item::Info(s) => s
            .lines()
            .flat_map(|l| wrap_line(&Line::from(Span::styled(l.to_string(), t.muted())), width))
            .collect(),
    }
}
