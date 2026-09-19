//! Line wrapping aware of spans and Unicode widths.

use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

pub fn width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

struct Builder {
    max: usize,
    lines: Vec<Line<'static>>,
    cur: Vec<Span<'static>>,
    cur_w: usize,
}

impl Builder {
    fn flush(&mut self) {
        let spans = std::mem::take(&mut self.cur);
        self.lines.push(Line::from(spans));
        self.cur_w = 0;
    }

    /// Break on overflow: trailing spaces are dropped.
    fn flush_wrap(&mut self) {
        while let Some(last) = self.cur.last_mut() {
            let trimmed = last.content.trim_end_matches(' ').to_string();
            if trimmed.is_empty() {
                self.cur.pop();
            } else {
                last.content = trimmed.into();
                break;
            }
        }
        self.flush();
    }

    fn push(&mut self, text: String, style: Style) {
        self.cur_w += width(&text);
        match self.cur.last_mut() {
            Some(last) if last.style == style => last.content.to_mut().push_str(&text),
            _ => self.cur.push(Span::styled(text, style)),
        }
    }
}

/// Splits a line into several of at most `max` columns. Breaks at spaces; a
/// word longer than `max` is broken by characters. Always returns at least
/// one line.
pub fn wrap_line(line: &Line<'static>, max: usize) -> Vec<Line<'static>> {
    let max = max.max(1);
    // tokens: (text, style, is_space)
    let mut tokens: Vec<(String, Style, bool)> = Vec::new();
    for span in &line.spans {
        let mut cur = String::new();
        let mut cur_space = false;
        for ch in span.content.chars() {
            let is_sp = ch == ' ';
            if !cur.is_empty() && is_sp != cur_space {
                tokens.push((std::mem::take(&mut cur), span.style, cur_space));
            }
            cur.push(ch);
            cur_space = is_sp;
        }
        if !cur.is_empty() {
            tokens.push((cur, span.style, cur_space));
        }
    }

    let mut b = Builder {
        max,
        lines: Vec::new(),
        cur: Vec::new(),
        cur_w: 0,
    };
    for (text, style, is_space) in tokens {
        let w = width(&text);
        if b.cur_w + w <= b.max {
            b.push(text, style);
            continue;
        }
        if is_space {
            // spaces at the break point are discarded
            if b.cur_w > 0 {
                b.flush_wrap();
            }
            continue;
        }
        if w > b.max {
            let mut piece = String::new();
            let mut pw = 0;
            for ch in text.chars() {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if b.cur_w + pw + cw > b.max {
                    if !piece.is_empty() {
                        b.push(std::mem::take(&mut piece), style);
                        pw = 0;
                    }
                    if b.cur_w > 0 {
                        b.flush_wrap();
                    }
                }
                piece.push(ch);
                pw += cw;
            }
            if !piece.is_empty() {
                b.push(piece, style);
            }
            continue;
        }
        b.flush_wrap();
        b.push(text, style);
    }
    if !b.cur.is_empty() || b.lines.is_empty() {
        b.flush();
    }
    let style = line.style;
    b.lines.into_iter().map(|l| l.style(style)).collect()
}

/// Pads with spaces up to `width` (for block backgrounds).
pub fn pad_line(mut line: Line<'static>, width: usize, style: Style) -> Line<'static> {
    let w = line.width();
    if w < width {
        line.spans.push(Span::styled(" ".repeat(width - w), style));
    }
    line
}

/// Truncates to `max` columns, appending `…` if needed.
pub fn truncate(s: &str, max: usize) -> String {
    if width(s) <= max {
        return s.to_string();
    }
    let mut out = String::new();
    let mut w = 0;
    for ch in s.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
        if w + cw > max.saturating_sub(1) {
            break;
        }
        out.push(ch);
        w += cw;
    }
    out.push('…');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(lines: &[Line<'static>]) -> Vec<String> {
        lines.iter().map(|l| l.to_string()).collect()
    }

    #[test]
    fn corta_por_palabras() {
        let l = Line::from("hola mundo cruel y grande");
        assert_eq!(
            texts(&wrap_line(&l, 10)),
            vec!["hola mundo", "cruel y", "grande"]
        );
    }

    #[test]
    fn palabra_larga_por_caracteres() {
        let l = Line::from("abcdefghijkl xy");
        assert_eq!(texts(&wrap_line(&l, 5)), vec!["abcde", "fghij", "kl xy"]);
    }

    #[test]
    fn conserva_estilos_y_anchos_dobles() {
        let l = Line::from(vec![
            Span::styled("ab ", Style::new().fg(ratatui::style::Color::Red)),
            Span::raw("日本語 x"),
        ]);
        let out = wrap_line(&l, 6);
        assert_eq!(texts(&out), vec!["ab", "日本語", "x"]);
        assert_eq!(out[0].spans[0].style.fg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn vacia_y_recorte() {
        assert_eq!(texts(&wrap_line(&Line::from(""), 8)), vec![""]);
        assert_eq!(truncate("hola mundo", 6), "hola …");
        assert_eq!(truncate("hola", 6), "hola");
        let p = pad_line(Line::from("ab"), 5, Style::new());
        assert_eq!(p.width(), 5);
    }
}
