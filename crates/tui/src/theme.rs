//! The ten tokens of the Moon system and their role in the TUI. No widget uses
//! a literal `Color`: everything goes through here.

use std::collections::BTreeMap;

use ratatui::style::{Color, Modifier, Style};

/// (token, hex, nearest xterm-256 index)
pub const TOKENS: [(&str, &str, u8); 10] = [
    ("night", "#1c1c29", 234),
    ("night-raised", "#2a2a3c", 236),
    ("night-line", "#4a4a60", 239),
    ("moon", "#8fb8ff", 111),
    ("moon-soft", "#c7dbff", 153),
    ("ink", "#f3ece3", 255),
    ("ink-muted", "#a9a7b8", 145),
    ("on-moon", "#1c1c29", 234),
    // status: soft green and red, outside the brand; only for the end of a request
    ("ok", "#8fd9a0", 115),
    ("alert", "#ff8f8f", 210),
];

#[derive(Clone, Debug, PartialEq)]
pub struct Theme {
    pub night: Color,
    pub night_raised: Color,
    pub night_line: Color,
    pub moon: Color,
    pub moon_soft: Color,
    pub ink: Color,
    pub ink_muted: Color,
    pub on_moon: Color,
    pub ok: Color,
    pub alert: Color,
    pub truecolor: bool,
}

pub fn parse_hex(s: &str) -> Option<Color> {
    let h = s.trim().strip_prefix('#')?;
    let v = match h.len() {
        6 => u32::from_str_radix(h, 16).ok()?,
        3 => {
            let v = u32::from_str_radix(h, 16).ok()?;
            let (r, g, b) = ((v >> 8) & 0xf, (v >> 4) & 0xf, v & 0xf);
            (r * 17) << 16 | (g * 17) << 8 | (b * 17)
        }
        _ => return None,
    };
    Some(Color::Rgb((v >> 16) as u8, (v >> 8) as u8, v as u8))
}

impl Theme {
    /// Moon palette in truecolor.
    pub fn moon() -> Self {
        let c = |i: usize| parse_hex(TOKENS[i].1).expect("valid hex in TOKENS");
        Self {
            night: c(0),
            night_raised: c(1),
            night_line: c(2),
            moon: c(3),
            moon_soft: c(4),
            ink: c(5),
            ink_muted: c(6),
            on_moon: c(7),
            ok: c(8),
            alert: c(9),
            truecolor: true,
        }
    }

    /// 256-color approximation for terminals without truecolor.
    pub fn moon_256() -> Self {
        let c = |i: usize| Color::Indexed(TOKENS[i].2);
        Self {
            night: c(0),
            night_raised: c(1),
            night_line: c(2),
            moon: c(3),
            moon_soft: c(4),
            ink: c(5),
            ink_muted: c(6),
            on_moon: c(7),
            ok: c(8),
            alert: c(9),
            truecolor: false,
        }
    }

    pub fn truecolor_supported() -> bool {
        matches!(
            std::env::var("COLORTERM").as_deref(),
            Ok("truecolor") | Ok("24bit")
        )
    }

    /// Builds the theme from the configuration: the full palette when the
    /// terminal announces truecolor, the 256-color approximation otherwise.
    /// Returns warnings (ignored overrides, invalid hex) to be shown, not to
    /// fail.
    pub fn resolve(overrides: &BTreeMap<String, String>) -> (Self, Vec<String>) {
        let truecolor = Self::truecolor_supported();
        let mut theme = if truecolor {
            Self::moon()
        } else {
            Self::moon_256()
        };
        let mut warnings = Vec::new();
        if !overrides.is_empty() && !truecolor {
            warnings.push("theme.overrides ignored: the terminal has no truecolor".to_string());
        } else {
            for (k, v) in overrides {
                match parse_hex(v) {
                    Some(c) if theme.set(k, c) => {}
                    Some(_) => warnings.push(format!("theme.overrides: unknown token `{k}`")),
                    None => {
                        warnings.push(format!("theme.overrides: `{k}` is not a valid hex color"))
                    }
                }
            }
        }
        (theme, warnings)
    }

    pub fn set(&mut self, token: &str, color: Color) -> bool {
        match token {
            "night" => self.night = color,
            "night-raised" => self.night_raised = color,
            "night-line" => self.night_line = color,
            "moon" => self.moon = color,
            "moon-soft" => self.moon_soft = color,
            "ink" => self.ink = color,
            "ink-muted" => self.ink_muted = color,
            "on-moon" => self.on_moon = color,
            "ok" => self.ok = color,
            "alert" => self.alert = color,
            _ => return false,
        }
        true
    }

    pub fn text(&self) -> Style {
        Style::new().fg(self.ink)
    }

    pub fn bold(&self) -> Style {
        Style::new().fg(self.ink).add_modifier(Modifier::BOLD)
    }

    pub fn muted(&self) -> Style {
        Style::new().fg(self.ink_muted)
    }

    pub fn accent(&self) -> Style {
        Style::new().fg(self.moon)
    }

    pub fn accent_bold(&self) -> Style {
        Style::new().fg(self.moon).add_modifier(Modifier::BOLD)
    }

    pub fn soft(&self) -> Style {
        Style::new().fg(self.moon_soft)
    }

    pub fn soft_bold(&self) -> Style {
        Style::new().fg(self.moon_soft).add_modifier(Modifier::BOLD)
    }

    /// Request completed: `✓ done`.
    pub fn ok(&self) -> Style {
        Style::new().fg(self.ok)
    }

    /// Request cancelled: `✗ cancelled`.
    pub fn alert(&self) -> Style {
        Style::new().fg(self.alert)
    }

    pub fn line(&self) -> Style {
        Style::new().fg(self.night_line)
    }

    /// Raised surface: code blocks, errors, the command suggestions.
    pub fn raised(&self) -> Style {
        Style::new().fg(self.ink).bg(self.night_raised)
    }

    pub fn raised_muted(&self) -> Style {
        Style::new().fg(self.ink_muted).bg(self.night_raised)
    }

    pub fn raised_soft(&self) -> Style {
        Style::new().fg(self.moon_soft).bg(self.night_raised)
    }

    pub fn raised_accent(&self) -> Style {
        Style::new().fg(self.moon).bg(self.night_raised)
    }

    /// Selected row: `moon` background, `on-moon` text.
    pub fn selected(&self) -> Style {
        Style::new().fg(self.on_moon).bg(self.moon)
    }

    /// Errors do not use `alert`: `✗` in `ink` over `night-raised`, as the Moon system dictates.
    pub fn error(&self) -> Style {
        self.raised()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex() {
        assert_eq!(parse_hex("#8fb8ff"), Some(Color::Rgb(0x8f, 0xb8, 0xff)));
        assert_eq!(parse_hex("#fff"), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(parse_hex("8fb8ff"), None);
        assert_eq!(parse_hex("#zzzzzz"), None);
    }

    #[test]
    fn overrides() {
        let mut o = BTreeMap::new();
        o.insert("moon".to_string(), "#ffffff".to_string());
        o.insert("nada".to_string(), "#000000".to_string());
        let (t, w) = Theme::resolve(&o);
        assert_eq!(w.len(), 1);
        // the palette follows the terminal: with no truecolor the overrides
        // do not apply and there is a second warning saying so
        if t.truecolor {
            assert_eq!(t.moon, Color::Rgb(255, 255, 255));
        } else {
            assert_eq!(t.moon, Color::Indexed(111));
        }
    }
}
