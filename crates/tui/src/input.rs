//! Own multi-line input box (`tui-textarea` does not compile with ratatui
//! 0.30). Grows up to `max_height` rows and then scrolls internally.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::UnicodeWidthChar;

use crate::theme::Theme;

pub const PROMPT: &str = "❯ ";
/// Columns the prompt takes (`❯` is 3 bytes in UTF-8, but 1 column).
pub const PROMPT_WIDTH: usize = 2;

#[derive(Debug, Clone)]
pub struct ChatInput {
    lines: Vec<String>,
    row: usize,
    /// Character index within the line.
    col: usize,
    pub max_height: u16,
    scroll: usize,
    /// Mouse selection over what is being written: where the drag started and
    /// where it is now, each as (line, character). Not ordered.
    sel: Option<(Pos, Pos)>,
}

/// A place in the text: logical line and character index within it.
type Pos = (usize, usize);

impl Default for ChatInput {
    fn default() -> Self {
        Self::new()
    }
}

fn cw(c: char) -> usize {
    UnicodeWidthChar::width(c).unwrap_or(0)
}

/// A visual row: text and where it comes from.
struct VRow {
    line: usize,
    /// Index of the row's first character within its line.
    start: usize,
    text: String,
}

impl ChatInput {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            row: 0,
            col: 0,
            max_height: 8,
            scroll: 0,
            sel: None,
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.lines.iter().all(|l| l.is_empty())
    }

    pub fn set_text(&mut self, s: &str) {
        self.lines = s
            .split('\n')
            .map(|l| l.trim_end_matches('\r').to_string())
            .collect();
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].chars().count();
    }

    pub fn clear(&mut self) {
        self.set_text("");
    }

    fn byte_idx(line: &str, col: usize) -> usize {
        line.char_indices()
            .nth(col)
            .map(|(i, _)| i)
            .unwrap_or(line.len())
    }

    fn cur_len(&self) -> usize {
        self.lines[self.row].chars().count()
    }

    pub fn insert_char(&mut self, c: char) {
        if c == '\n' {
            self.newline();
            return;
        }
        if c == '\r' || (c.is_control() && c != '\t') {
            return;
        }
        let line = &mut self.lines[self.row];
        let i = Self::byte_idx(line, self.col);
        line.insert(i, c);
        self.col += 1;
    }

    /// Inserts pasted text; line breaks do not send.
    pub fn insert_str(&mut self, s: &str) {
        for c in s.chars() {
            match c {
                '\r' => {}
                '\n' => self.newline(),
                '\t' => {
                    self.insert_char(' ');
                    self.insert_char(' ');
                }
                c => self.insert_char(c),
            }
        }
    }

    pub fn newline(&mut self) {
        let line = &mut self.lines[self.row];
        let i = Self::byte_idx(line, self.col);
        let rest = line.split_off(i);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        if self.col > 0 {
            let line = &mut self.lines[self.row];
            let i = Self::byte_idx(line, self.col - 1);
            line.remove(i);
            self.col -= 1;
        } else if self.row > 0 {
            let cur = self.lines.remove(self.row);
            self.row -= 1;
            self.col = self.cur_len();
            self.lines[self.row].push_str(&cur);
        }
    }

    pub fn delete(&mut self) {
        if self.col < self.cur_len() {
            let line = &mut self.lines[self.row];
            let i = Self::byte_idx(line, self.col);
            line.remove(i);
        } else if self.row + 1 < self.lines.len() {
            let next = self.lines.remove(self.row + 1);
            self.lines[self.row].push_str(&next);
        }
    }

    pub fn delete_word_back(&mut self) {
        if self.col == 0 {
            self.backspace();
            return;
        }
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        let mut i = self.col;
        while i > 0 && chars[i - 1] == ' ' {
            i -= 1;
        }
        while i > 0 && chars[i - 1] != ' ' {
            i -= 1;
        }
        let new: String = chars[..i].iter().chain(chars[self.col..].iter()).collect();
        self.lines[self.row] = new;
        self.col = i;
    }

    pub fn kill_to_start(&mut self) {
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        self.lines[self.row] = chars[self.col..].iter().collect();
        self.col = 0;
    }

    pub fn kill_to_end(&mut self) {
        let chars: Vec<char> = self.lines[self.row].chars().collect();
        self.lines[self.row] = chars[..self.col].iter().collect();
    }

    pub fn left(&mut self) {
        if self.col > 0 {
            self.col -= 1;
        } else if self.row > 0 {
            self.row -= 1;
            self.col = self.cur_len();
        }
    }

    pub fn right(&mut self) {
        if self.col < self.cur_len() {
            self.col += 1;
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = 0;
        }
    }

    pub fn up(&mut self) {
        if self.row > 0 {
            self.row -= 1;
            self.col = self.col.min(self.cur_len());
        }
    }

    pub fn down(&mut self) {
        if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.col = self.col.min(self.cur_len());
        }
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.cur_len();
    }

    pub fn on_first_line(&self) -> bool {
        self.row == 0
    }

    pub fn on_last_line(&self) -> bool {
        self.row + 1 == self.lines.len()
    }

    /// Visual rows (text broken by characters at `text_w`) and the cursor's
    /// visual position.
    fn visual(&self, text_w: usize) -> (Vec<String>, (usize, usize)) {
        let (rows, cursor) = self.layout(text_w);
        (rows.into_iter().map(|r| r.text).collect(), cursor)
    }

    /// Like `visual`, but each row knows which logical line it comes from and
    /// at which character of it it starts (to highlight ranges even when they
    /// are broken).
    fn layout(&self, text_w: usize) -> (Vec<VRow>, (usize, usize)) {
        let text_w = text_w.max(1);
        let mut rows: Vec<VRow> = Vec::new();
        let mut cursor = (0, 0);
        for (li, line) in self.lines.iter().enumerate() {
            let mut cur = String::new();
            let mut w = 0;
            let mut start = 0;
            let mut cursor_set = false;
            for (ci, ch) in line.chars().enumerate() {
                let c = cw(ch);
                if w + c > text_w {
                    rows.push(VRow {
                        line: li,
                        start,
                        text: std::mem::take(&mut cur),
                    });
                    start = ci;
                    w = 0;
                }
                if li == self.row && ci == self.col {
                    cursor = (w, rows.len());
                    cursor_set = true;
                }
                cur.push(ch);
                w += c;
            }
            if li == self.row && !cursor_set {
                // cursor at the end of the line; if it exactly fills the row, it moves down
                if w >= text_w && w > 0 {
                    rows.push(VRow {
                        line: li,
                        start,
                        text: std::mem::take(&mut cur),
                    });
                    start = line.chars().count();
                    w = 0;
                }
                cursor = (w, rows.len());
            }
            rows.push(VRow {
                line: li,
                start,
                text: cur,
            });
        }
        (rows, cursor)
    }

    /// Click in the box: `x`/`y` relative to the corner of the area drawn in
    /// the last `render` (`width` is the total width of that area, prompt
    /// included). Moves the cursor to the character under the click; past the
    /// end of a visual row, to the end of that row; below the text, to the
    /// very end.
    pub fn click(&mut self, width: u16, x: u16, y: u16) {
        let text_w = (width as usize).saturating_sub(PROMPT_WIDTH).max(1);
        let vx = (x as usize).saturating_sub(PROMPT_WIDTH);
        let vy = y as usize + self.scroll;
        // Same walk as `visual`, including the phantom row that appears when
        // the cursor's line exactly fills its last row.
        let mut vrow = 0usize;
        for (li, line) in self.lines.iter().enumerate() {
            let mut w = 0;
            let mut len = 0;
            for (ci, ch) in line.chars().enumerate() {
                let c = cw(ch);
                if w + c > text_w {
                    if vrow == vy {
                        // click to the right of the end of this visual row
                        self.row = li;
                        self.col = ci;
                        return;
                    }
                    vrow += 1;
                    w = 0;
                }
                if vrow == vy && vx < w + c {
                    self.row = li;
                    self.col = ci;
                    return;
                }
                w += c;
                len = ci + 1;
            }
            let phantom = li == self.row && self.col >= len && w >= text_w && w > 0;
            if vrow == vy || (phantom && vrow + 1 == vy) {
                self.row = li;
                self.col = len;
                return;
            }
            vrow += 1 + usize::from(phantom);
        }
        // below all the text
        self.row = self.lines.len() - 1;
        self.col = self.cur_len();
    }

    // ----- mouse selection --------------------------------------------------

    /// Starts a selection where the click lands; the cursor goes there too.
    pub fn select_from(&mut self, width: u16, x: u16, y: u16) {
        self.click(width, x, y);
        let at = (self.row, self.col);
        self.sel = Some((at, at));
    }

    /// Drags the open selection to where the mouse is.
    pub fn select_to(&mut self, width: u16, x: u16, y: u16) {
        if self.sel.is_none() {
            return;
        }
        self.click(width, x, y);
        if let Some((_, head)) = self.sel.as_mut() {
            *head = (self.row, self.col);
        }
    }

    pub fn has_selection(&self) -> bool {
        self.sel.is_some()
    }

    pub fn clear_selection(&mut self) {
        self.sel = None;
    }

    /// The selection, ordered: (start, end), end excluded.
    fn bounds(&self) -> Option<(Pos, Pos)> {
        let (a, b) = self.sel?;
        Some(if a <= b { (a, b) } else { (b, a) })
    }

    /// Text under the selection, empty if it is only a caret.
    pub fn selection_text(&self) -> String {
        let Some(((r1, c1), (r2, c2))) = self.bounds() else {
            return String::new();
        };
        let take = |line: &str, from: usize, to: usize| -> String {
            line.chars()
                .skip(from)
                .take(to.saturating_sub(from))
                .collect()
        };
        if r1 == r2 {
            return take(&self.lines[r1], c1, c2);
        }
        let mut out = vec![take(&self.lines[r1], c1, usize::MAX)];
        for line in &self.lines[r1 + 1..r2] {
            out.push(line.clone());
        }
        out.push(take(&self.lines[r2], 0, c2));
        out.join("\n")
    }

    /// Whether the character `col` of `line` falls inside the selection.
    fn is_selected(&self, line: usize, col: usize) -> bool {
        let Some(((r1, c1), (r2, c2))) = self.bounds() else {
            return false;
        };
        if line < r1 || line > r2 {
            return false;
        }
        let from = if line == r1 { c1 } else { 0 };
        let to = if line == r2 { c2 } else { usize::MAX };
        col >= from && col < to
    }

    /// Height it needs for `width` total columns (prompt included).
    pub fn height(&self, width: u16) -> u16 {
        let text_w = (width as usize).saturating_sub(PROMPT_WIDTH).max(1);
        let (rows, _) = self.visual(text_w);
        (rows.len() as u16).clamp(1, self.max_height.max(1))
    }

    /// Draws and returns the cursor's absolute position. The first `cmd_chars`
    /// characters of the first line are drawn as a command.
    pub fn render(
        &mut self,
        area: Rect,
        buf: &mut Buffer,
        theme: &Theme,
        cmd_chars: usize,
    ) -> (u16, u16) {
        let text_w = (area.width as usize).saturating_sub(PROMPT_WIDTH).max(1);
        let (rows, (cx, cy)) = self.layout(text_w);
        let h = area.height.max(1) as usize;
        if cy < self.scroll {
            self.scroll = cy;
        } else if cy >= self.scroll + h {
            self.scroll = cy + 1 - h;
        }
        if self.scroll + h > rows.len() {
            self.scroll = rows.len().saturating_sub(h);
        }
        let lines: Vec<Line> = rows
            .iter()
            .skip(self.scroll)
            .take(h)
            .enumerate()
            .map(|(i, r)| {
                let prompt = if i + self.scroll == 0 { PROMPT } else { "  " };
                let mut spans = vec![Span::styled(prompt, theme.accent())];
                // one style per character — command prefix, selection, plain
                // text — and consecutive characters of the same style joined
                let mut run = String::new();
                let mut run_style = None;
                for (ci, ch) in r.text.chars().enumerate() {
                    let col = r.start + ci;
                    let style = if self.is_selected(r.line, col) {
                        theme.selected()
                    } else if r.line == 0 && col < cmd_chars {
                        theme.accent_bold()
                    } else {
                        theme.text()
                    };
                    if run_style != Some(style) {
                        if let Some(st) = run_style {
                            spans.push(Span::styled(std::mem::take(&mut run), st));
                        }
                        run_style = Some(style);
                    }
                    run.push(ch);
                }
                if let Some(st) = run_style {
                    spans.push(Span::styled(run, st));
                }
                Line::from(spans)
            })
            .collect();
        Paragraph::new(lines).render(area, buf);
        let x = area.x + PROMPT_WIDTH as u16 + cx.min(u16::MAX as usize) as u16;
        let y = area.y + (cy - self.scroll) as u16;
        (x.min(area.right().saturating_sub(1)), y)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seleccion_con_el_raton() {
        let mut i = ChatInput::new();
        i.insert_str("hola mundo");
        // «mundo»: de la columna 5 a la 10, con el prompt de por medio
        i.select_from(20, PROMPT_WIDTH as u16 + 5, 0);
        i.select_to(20, PROMPT_WIDTH as u16 + 10, 0);
        assert!(i.has_selection());
        assert_eq!(i.selection_text(), "mundo");
        // arrastrar hacia atrás da lo mismo
        i.select_from(20, PROMPT_WIDTH as u16 + 10, 0);
        i.select_to(20, PROMPT_WIDTH as u16 + 5, 0);
        assert_eq!(i.selection_text(), "mundo");
        // un clic sin arrastre no selecciona nada
        i.select_from(20, PROMPT_WIDTH as u16 + 2, 0);
        assert_eq!(i.selection_text(), "");
        i.clear_selection();
        assert!(!i.has_selection());
        assert_eq!(i.selection_text(), "");
    }

    #[test]
    fn la_seleccion_cruza_lineas() {
        let mut i = ChatInput::new();
        i.set_text("una\ndos\ntres");
        i.select_from(20, PROMPT_WIDTH as u16 + 1, 0);
        i.select_to(20, PROMPT_WIDTH as u16 + 2, 2);
        assert_eq!(i.selection_text(), "na\ndos\ntr");
        // y lo que se ve seleccionado son esos caracteres, no otros
        assert!(!i.is_selected(0, 0));
        assert!(i.is_selected(0, 1));
        assert!(i.is_selected(1, 0));
        assert!(i.is_selected(2, 1));
        assert!(!i.is_selected(2, 2));
    }

    #[test]
    fn edicion_basica() {
        let mut i = ChatInput::new();
        i.insert_str("hola");
        i.newline();
        i.insert_str("mundo");
        assert_eq!(i.text(), "hola\nmundo");
        assert!(!i.on_first_line());
        i.backspace();
        i.backspace();
        assert_eq!(i.text(), "hola\nmun");
        i.home();
        i.backspace();
        assert_eq!(i.text(), "holamun");
        i.left();
        i.left();
        i.delete();
        assert_eq!(i.text(), "hoamun");
        i.set_text("a b  c");
        i.delete_word_back();
        assert_eq!(i.text(), "a b  ");
        i.delete_word_back();
        assert_eq!(i.text(), "a ");
        i.clear();
        assert!(i.is_empty());
    }

    #[test]
    fn pegado_y_altura() {
        let mut i = ChatInput::new();
        i.insert_str("uno\r\ndos\tx");
        assert_eq!(i.text(), "uno\ndos  x");
        assert_eq!(i.height(80), 2);
        i.set_text(&"a".repeat(20));
        // the line exactly fills two rows: the cursor moves down to a third
        assert_eq!(i.height(12), 3);
        let (rows, cursor) = i.visual(10);
        assert_eq!(rows, vec!["aaaaaaaaaa", "aaaaaaaaaa", ""]);
        assert_eq!(cursor, (0, 2));
        i.left();
        let (_, cursor) = i.visual(10);
        assert_eq!(cursor, (9, 1));
        i.set_text(&"a".repeat(200));
        assert_eq!(i.height(12), 8);
    }

    #[test]
    fn el_cursor_va_pegado_al_texto() {
        use ratatui::buffer::Buffer;
        use ratatui::layout::Rect;
        let mut i = ChatInput::new();
        i.insert_str("hola");
        let area = Rect::new(0, 5, 40, 1);
        let mut buf = Buffer::empty(area);
        let (x, y) = i.render(area, &mut buf, &Theme::moon(), 0);
        assert_eq!((x, y), (PROMPT_WIDTH as u16 + 4, 5));
        assert_eq!(buf[(0, 5)].symbol(), "❯");
        assert_eq!(unicode_width::UnicodeWidthStr::width(PROMPT), PROMPT_WIDTH);
        assert_eq!(buf[(2, 5)].symbol(), "h");
        // the usable width is also measured in columns: 40 - 2 = 38
        i.set_text(&"a".repeat(38));
        assert_eq!(i.height(40), 2);
        i.set_text(&"a".repeat(37));
        assert_eq!(i.height(40), 1);
    }

    #[test]
    fn clic_mueve_el_cursor() {
        let mut i = ChatInput::new();
        i.set_text("hola mundo\nadiós");
        // width 12 → 10 text columns: "hola mundo" fits exactly in one row
        i.click(12, PROMPT_WIDTH as u16 + 5, 0);
        assert_eq!((i.row, i.col), (0, 5));
        assert_eq!(i.visual(10).1, (5, 0));
        // on the prompt → start of the line
        i.click(12, 0, 0);
        assert_eq!((i.row, i.col), (0, 0));
        // past the end of the row → end of that row
        i.click(12, 40, 1);
        assert_eq!((i.row, i.col), (1, 5));
        // below the text → the very end
        i.click(12, 3, 9);
        assert_eq!((i.row, i.col), (1, 5));

        // long line broken into visual rows
        i.set_text("abcdefghijklmnopqrstuvwxyz");
        i.click(12, PROMPT_WIDTH as u16 + 3, 1);
        assert_eq!((i.row, i.col), (0, 13));
        assert_eq!(i.visual(10).1, (3, 1));
        // end of the first visual row (spare column) → before the 'k'
        i.click(12, 30, 0);
        assert_eq!((i.row, i.col), (0, 10));

        // double widths: the character's second cell also selects it
        i.set_text("日本語");
        i.click(12, PROMPT_WIDTH as u16 + 3, 0);
        assert_eq!((i.row, i.col), (0, 1));

        // phantom row: the line fills the row and the cursor is at the end
        i.set_text(&"a".repeat(20));
        assert_eq!(i.visual(10).0.len(), 3);
        i.click(12, 5, 2);
        assert_eq!((i.row, i.col), (0, 20));
        i.click(12, 40, 1);
        assert_eq!((i.row, i.col), (0, 20));
        // with the cursor in the middle there is no phantom row: row 2 no longer exists
        i.click(12, PROMPT_WIDTH as u16 + 3, 0);
        assert_eq!((i.row, i.col), (0, 3));
        assert_eq!(i.visual(10).0.len(), 2);
        i.click(12, 5, 2);
        assert_eq!((i.row, i.col), (0, 20));

        // with internal scroll: the box shows rows 2..4 and the click is offset
        let mut i = ChatInput::new();
        i.max_height = 2;
        i.set_text("uno\ndos\ntres\ncuatro");
        let area = Rect::new(0, 0, 12, 2);
        let mut buf = Buffer::empty(area);
        i.render(area, &mut buf, &Theme::moon(), 0);
        assert_eq!(i.scroll, 2);
        i.click(12, PROMPT_WIDTH as u16 + 1, 0);
        assert_eq!((i.row, i.col), (2, 1));
    }

    #[test]
    fn el_comando_se_resalta_aunque_se_corte() {
        use ratatui::style::Modifier;
        let t = Theme::moon();
        let mut i = ChatInput::new();
        i.set_text("/model qwen");
        let area = Rect::new(0, 0, 12, 3); // 10 text columns: "/model qwe" + "n"
        let is_cmd = |buf: &Buffer, x: u16, y: u16| {
            let st = buf[(x, y)].style();
            st.fg == Some(t.moon) && st.add_modifier.contains(Modifier::BOLD)
        };
        let mut buf = Buffer::empty(area);
        i.render(area, &mut buf, &t, 6);
        assert!(is_cmd(&buf, 2, 0) && is_cmd(&buf, 7, 0));
        assert!(!is_cmd(&buf, 8, 0) && !is_cmd(&buf, 2, 1));
        // a command longer than the row stays highlighted on the next one
        i.set_text("/abcdefghijklm x");
        let mut buf = Buffer::empty(area);
        i.render(area, &mut buf, &t, 14);
        assert!(is_cmd(&buf, 11, 0) && is_cmd(&buf, 2, 1) && is_cmd(&buf, 5, 1));
        assert!(!is_cmd(&buf, 6, 1));
        // with no command range nothing is highlighted
        let mut buf = Buffer::empty(area);
        i.render(area, &mut buf, &t, 0);
        assert!(!is_cmd(&buf, 2, 0));
    }

    #[test]
    fn cursor_con_anchos_dobles() {
        let mut i = ChatInput::new();
        i.insert_str("日本");
        let (rows, cursor) = i.visual(10);
        assert_eq!(rows, vec!["日本"]);
        assert_eq!(cursor, (4, 0));
        i.left();
        let (_, cursor) = i.visual(10);
        assert_eq!(cursor, (2, 0));
    }
}
