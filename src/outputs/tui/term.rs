//! Terminal primitives for the std-only curses-style UI.
//!
//! Raw mode, window size, and alternate screen via direct libc FFI
//! (same pattern as `platform/linux/statvfs.rs`). ANSI escapes do the
//! drawing — no `curses`/`terminfo` needed on modern terminals.

use crate::cli::args::Args;

/// Linux `termios` (glibc/musl, x86_64 + aarch64): 4×u32 flags, line
/// byte, 32 control chars, two speeds = 60 bytes.
#[repr(C)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 32],
    c_ispeed: u32,
    c_ospeed: u32,
}

const _: () = assert!(std::mem::size_of::<Termios>() == 60);

// c_cc indices.
const VMIN: usize = 6;
const VTIME: usize = 5;

// Flag bits (Linux/glibc, stable ABI).
const BRKINT: u32 = 0x2;
const ICRNL: u32 = 0x100;
const INPCK: u32 = 0x10;
const ISTRIP: u32 = 0x20;
const IXON: u32 = 0x400;
const OPOST: u32 = 0x1;
const CS8: u32 = 0x30;
const ECHO: u32 = 0x8;
const ICANON: u32 = 0x2;
const IEXTEN: u32 = 0x8000;
const ISIG: u32 = 0x1;

/// `winsize` for `TIOCGWINSZ`.
#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    _xpixel: u16,
    _ypixel: u16,
}

extern "C" {
    fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, action: i32, termios: *const Termios) -> i32;
    fn ioctl(fd: i32, request: u64, ...) -> i32;
}

const STDIN_FD: i32 = 0;
const TCSANOW: i32 = 0;
const TIOCGWINSZ: u64 = 0x5413;

fn apply(raw: &Termios) -> std::io::Result<()> {
    let rc = unsafe { tcsetattr(STDIN_FD, TCSANOW, raw) };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Guard that puts stdin into raw mode on creation and restores the
/// original mode on drop (including on panic unwind).
pub struct RawMode {
    orig: Termios,
}

impl RawMode {
    pub fn enter() -> std::io::Result<Self> {
        let mut orig: Termios = unsafe { std::mem::zeroed() };
        if unsafe { tcgetattr(STDIN_FD, &mut orig) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut raw: Termios = unsafe { std::ptr::read(&orig) };
        raw.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
        raw.c_oflag &= !OPOST;
        raw.c_cflag |= CS8;
        // No echo, no canonical mode, no signals (Ctrl-C arrives as 0x03
        // and quits cleanly through the key parser instead of killing
        // the process with the terminal left raw).
        raw.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
        raw.c_cc[VMIN] = 1;
        raw.c_cc[VTIME] = 0;
        apply(&raw)?;
        Ok(Self { orig })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = apply(&self.orig);
    }
}

/// Current terminal size in (rows, cols). Falls back to 24×80 when the
/// ioctl fails (piped output, dumb terminals).
pub fn size() -> (u16, u16) {
    let mut ws = Winsize { ws_row: 0, ws_col: 0, _xpixel: 0, _ypixel: 0 };
    let rc = unsafe { ioctl(STDIN_FD, TIOCGWINSZ, &mut ws) };
    if rc == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
        (ws.ws_row, ws.ws_col)
    } else {
        (24, 80)
    }
}

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
