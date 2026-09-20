//! Snapshot → styled screen text for the TUI.
//!
//! Every registered plugin appears: headline plugins (system, cpu,
//! mem, load, network, disk I/O, filesystems, sensors, processes) get
//! bars and tables; the rest render as compact generic sections. All
//! drawing honors the CLI display toggles (`Style`) so `--disable-*`
//! flags behave the same as upstream curses options.
//!
//! Layout: `overview` (headline sections), `tables` (array plugins +
//! process list), `frame` (generic sections + frame assembly). Pure
//! functions over `Value` snapshots — no TTY needed, fully unit tested.

use std::collections::BTreeMap;

use super::term::{pct_color, Style};
use crate::cli::args::Args;
use crate::core::value::Value;

pub(crate) mod frame;
pub(crate) mod overview;
pub(crate) mod tables;

pub(crate) use frame::render;

/// Frame options: toggles + terminal size.
#[derive(Debug, Clone)]
pub struct RenderOpts {
    pub style: Style,
    pub cols: usize,
    pub fahrenheit: bool,
    pub byte_units: bool,
    pub percpu: bool,
    pub programs: bool,
    pub sparkline: bool,
    pub sort_key: String,
    pub hide_kernel_threads: bool,
    pub process_short_name: bool,
    pub hide_public_info: bool,
    pub focus: Vec<String>,
    pub separator: bool,
    pub fs_free_space: bool,
}

impl RenderOpts {
    pub fn from_args(args: &Args, cols: usize) -> Self {
        Self {
            style: Style::from_args(args),
            cols,
            fahrenheit: args.fahrenheit,
            byte_units: args.byte_units,
            percpu: args.percpu,
            programs: args.programs,
            sparkline: args.sparkline,
            sort_key: args.sort_processes.clone().unwrap_or_else(|| "cpu_percent".into()),
            hide_kernel_threads: args.hide_kernel_threads,
            process_short_name: args.process_short_name,
            hide_public_info: args.hide_public_info,
            focus: args
                .process_focus
                .as_deref()
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            separator: args.enable_separator,
            fs_free_space: args.fs_free_space,
        }
    }
}

/// Per-frame UI state (cursor, overlays, runtime toggles).
#[derive(Debug, Clone)]
pub struct UiState {
    pub selected: usize,
    pub show_help: bool,
    pub percpu: bool,
}

impl UiState {
    pub fn new(percpu: bool) -> Self {
        Self { selected: 0, show_help: false, percpu }
    }
}

pub(crate) fn plugin_obj<'a>(snap: &'a Value, name: &str) -> Option<&'a BTreeMap<String, Value>> {
    snap.as_object()?.get(name)?.as_object()
}

pub(crate) fn num(snap: &Value, plugin: &str, key: &str) -> f64 {
    plugin_obj(snap, plugin)
        .and_then(|o| o.get(key))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

pub(crate) fn text(snap: &Value, plugin: &str, key: &str) -> String {
    match plugin_obj(snap, plugin).and_then(|o| o.get(key)) {
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.as_f64().map(|n| format!("{}", n)).unwrap_or_default(),
        None => String::new(),
    }
}

/// Human byte size: B/KB/MB/GB (1024-based, upstream `pretty` parity).
pub fn fmt_bytes(n: f64) -> String {
    let units = ["B", "KB", "MB", "GB", "TB"];
    let mut v = n.max(0.0);
    let mut u = 0;
    while v >= 1024.0 && u + 1 < units.len() {
        v /= 1024.0;
        u += 1;
    }
    if u == 0 {
        format!("{}{}", v as u64, units[u])
    } else {
        format!("{:.1}{}", v, units[u])
    }
}

/// Rate formatting: bits/s by default, bytes/s with `--byte`.
pub fn fmt_rate(opts: &RenderOpts, bytes_per_sec: f64) -> String {
    if opts.byte_units {
        format!("{}/s", fmt_bytes(bytes_per_sec))
    } else {
        let bits = bytes_per_sec * 8.0;
        let units = ["b", "Kb", "Mb", "Gb"];
        let mut v = bits.max(0.0);
        let mut u = 0;
        while v >= 1000.0 && u + 1 < units.len() {
            v /= 1000.0;
            u += 1;
        }
        format!("{:.1}{}/s", v, units[u])
    }
}

/// Temperature: Celsius by default, Fahrenheit with `--fahrenheit`.
pub fn fmt_temp(opts: &RenderOpts, celsius: f64) -> String {
    if opts.fahrenheit {
        format!("{:.0}F", celsius * 9.0 / 5.0 + 32.0)
    } else {
        format!("{:.0}C", celsius)
    }
}

pub(crate) fn title_bar(style: &Style, cols: usize, name: &str) -> String {
    let plain = format!(" {} ", name.to_uppercase());
    let mut line = style.b(&plain);
    // Pad with dashes (visible width math ignores ANSI escapes).
    let pad = cols.saturating_sub(plain.chars().count());
    line.push_str(&"-".repeat(pad));
    line
}

/// Truncate a line to the terminal width. ANSI escape sequences pass
/// through without counting toward the visible width.
pub fn fit(s: &str, cols: usize) -> String {
    let mut out = String::new();
    let mut visible = 0;
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            // Copy escape sequences verbatim (they carry no width).
            out.push(c);
            while let Some(d) = chars.next() {
                out.push(d);
                if d.is_ascii_alphabetic() {
                    break;
                }
            }
            continue;
        }
        if visible >= cols {
            break;
        }
        out.push(c);
        visible += 1;
    }
    out
}

pub(crate) fn pct_styled(style: &Style, pct: f64) -> String {
    style.fg(pct_color(pct), &format!("{:5.1}%", pct.clamp(0.0, 100.0)))
}

pub(crate) fn section_head(opts: &RenderOpts, name: &str) -> Vec<String> {
    if opts.separator {
        vec![title_bar(&opts.style, opts.cols, name)]
    } else {
        vec![opts.style.b(&name.to_uppercase())]
    }
}
