//! Clipboard: `arboard` when available; otherwise OSC 52 through the terminal
//! itself (works over SSH).

use std::io::Write;

use base64::Engine;

/// Copies and returns which route was used.
pub fn copy(text: &str) -> Result<&'static str, String> {
    #[cfg(feature = "clipboard")]
    {
        match arboard::Clipboard::new().and_then(|mut c| c.set_text(text.to_string())) {
            Ok(()) => return Ok("clipboard"),
            Err(e) => tracing::debug!(error = %e, "arboard no disponible, probando OSC 52"),
        }
    }
    osc52(text).map(|_| "OSC 52")
}

fn osc52(text: &str) -> Result<(), String> {
    let b64 = base64::engine::general_purpose::STANDARD.encode(text);
    let seq = format!("\x1b]52;c;{b64}\x07");
    let mut out = std::io::stdout();
    out.write_all(seq.as_bytes())
        .and_then(|_| out.flush())
        .map_err(|e| e.to_string())
}
