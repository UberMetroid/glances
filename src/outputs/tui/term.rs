//! Terminal styling for the std-only curses-style UI.
//!
//! Raw mode, window size, and polled input live in
//! `platform::linux::tty` (all `unsafe` per AC-11); this module keeps
//! the pure ANSI styling plus re-exports so `tui::term` stays the
//! single import point for the UI.

use crate::cli::args::Args;

pub use crate::platform::linux::tty::{size, RawMode};

pub const ALT_ENTER: &str = "\x1b[?1049h";
pub const ALT_EXIT: &str = "\x1b[?1049l";
pub const HIDE_CURSOR: &str = "\x1b[?25l";
pub const SHOW_CURSOR: &str = "\x1b[?25h";
pub const HOME: &str = "\x1b[H";
pub const ERASE_DOWN: &str = "\x1b[J";

/// Drawing style derived from CLI display toggles (upstream parity).
#[derive(Debug, Clone)]
pub struct Style {
    pub bold: bool,
    pub color: bool,
    pub unicode: bool,
}

impl Style {
    pub fn from_args(args: &Args) -> Self {
        Self {
            bold: !args.disable_bold,
            color: !args.disable_bg,
            unicode: !args.disable_unicode,
        }
    }

    /// Wrap in an ANSI color code (`31`–`37`, `90`–`97`), or plain text.
    pub fn fg(&self, code: &str, s: &str) -> String {
        if self.color {
            format!("\x1b[{}m{}\x1b[0m", code, s)
        } else {
            s.to_string()
        }
    }

    /// Bold wrapper, or plain text with `--disable-bold`.
    pub fn b(&self, s: &str) -> String {
        if self.bold {
            format!("\x1b[1m{}\x1b[0m", s)
        } else {
            s.to_string()
        }
    }

    /// Inverse video (process selection cursor), or `> ` marker fallback.
    pub fn inv(&self, s: &str) -> String {
        if self.color {
            format!("\x1b[7m{}\x1b[0m", s)
        } else {
            format!("> {}", s)
        }
    }
}

/// Horizontal usage bar. Unicode blocks by default, ASCII fallback with
/// `--disable-unicode`.
pub fn bar(style: &Style, pct: f64, width: usize) -> String {
    let pct = pct.clamp(0.0, 100.0);
    let filled = ((pct / 100.0) * width as f64).round() as usize;
    let filled = filled.min(width);
    if style.unicode {
        format!("{}{}", "█".repeat(filled), "░".repeat(width - filled))
    } else {
        format!("[{}{}]", "#".repeat(filled), "-".repeat(width - filled))
    }
}

/// Single-cell sparkline block for a percentage (`--sparkline` shows
/// these instead of bars). ASCII `#` fallback with `--disable-unicode`.
pub fn spark(style: &Style, pct: f64) -> String {
    const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let pct = pct.clamp(0.0, 100.0);
    if !style.unicode {
        return "#".to_string();
    }
    let i = ((pct / 100.0) * 7.0).round() as usize;
    BLOCKS[i.min(7)].to_string()
}

/// Color name for a percentage (fixed 70/90 bands; upstream's
/// per-plugin careful/warning/critical limits feed the API, not the TUI).
pub fn pct_color(pct: f64) -> &'static str {
    if pct >= 90.0 {
        "31"
    } else if pct >= 70.0 {
        "33"
    } else {
        "32"
    }
}
