//! Runtime hotkey dispatch (upstream `_hotkeys` table + the `catch_*`
//! handlers in `glances_curses.py` parity).
//!
//! Event flow lives here (filter-edit mode, kill/nice confirmation,
//! arrow routing); the byte table itself is in `hotkeys`.

use super::hotkeys::handle_byte;
use super::keys::Key;
use super::render::tables::process_rows;
use super::render::{RenderOpts, UiState};
use crate::cli::args::Args;
use crate::core::stats::GlancesStats;

/// Runtime-toggle plugin sets (upstream curses section lists).
pub(crate) const SIDEBAR: &[&str] = &[
    "network", "ports", "wifi", "connections", "diskio", "fs", "irq",
    "folders", "raid", "smart", "sensors", "now",
];
pub(crate) const FULL_QUICKLOOK_OFF: &[&str] = &["cpu", "npu", "mpp", "gpu", "mem", "memswap"];
pub(crate) const TOP_MENU: &[&str] = &["quicklook", "cpu", "mem", "memswap", "load"];
pub(crate) const PROCESS_GROUP: &[&str] = &["processcount", "processlist", "programlist"];

/// Sort keys cycled by the arrow keys (upstream `_sort_loop` order,
/// restricted to keys the renderer understands).
pub(crate) const SORT_CYCLE: &[&str] = &[
    "cpu_percent", "memory_percent", "name", "cpu_times",
    "io_counters", "cpu_num", "username", "auto",
];

/// Handle one key. Returns true when the UI should quit.
pub fn handle_key(
    key: Key,
    ui: &mut UiState,
    opts: &mut RenderOpts,
    stats: &GlancesStats,
    args: &Args,
) -> bool {
    // Filter-edit mode eats everything except ESC/Ctrl-C/Enter.
    if ui.filter_input.is_some() {
        return handle_filter_key(key, ui, stats);
    }
    // An armed kill/nice only accepts its confirming key.
    if let Some(armed) = ui.confirm {
        match key {
            Key::Byte(b) if super::hotkeys::confirms(armed, b) => {
                super::hotkeys::execute_confirm(armed, opts, stats)
            }
            Key::Quit | Key::Interrupt => {}
            _ => {}
        }
        ui.confirm = None;
        return false;
    }
    match key {
        Key::Quit | Key::Interrupt => true,
        Key::Up => {
            ui.selected = ui.selected.saturating_sub(1);
            false
        }
        Key::Down => {
            ui.selected = ui.selected.saturating_add(1);
            false
        }
        Key::Enter => {
            ui.filter_input = Some(String::new());
            false
        }
        Key::Refresh => {
            ui.refresh_now = true;
            false
        }
        Key::Left | Key::Right | Key::ShiftLeft | Key::ShiftRight => {
            handle_horizontal(key, ui, opts, args);
            false
        }
        Key::Byte(b) => handle_byte(b, ui, opts, stats),
    }
}

/// Keys inside the filter prompt. ESC cancels, Enter applies.
fn handle_filter_key(key: Key, ui: &mut UiState, stats: &GlancesStats) -> bool {
    let buf = ui.filter_input.as_mut().expect("filter mode");
    match key {
        Key::Enter => {
            let text = std::mem::take(buf);
            ui.filter_input = None;
            let raw = if text.is_empty() { None } else { Some(text.as_str()) };
            if let Ok(mut guard) = stats.plugins.write() {
                for p in guard.iter_mut() {
                    p.set_process_filter(raw);
                }
            }
            false
        }
        Key::Quit => {
            ui.filter_input = None;
            false
        }
        Key::Interrupt => true,
        Key::Byte(0x7f) | Key::Byte(0x08) => {
            buf.pop();
            false
        }
        Key::Byte(b) if b.is_ascii_graphic() || b == b' ' => {
            buf.push(b as char);
            false
        }
        _ => false,
    }
}

/// Left/right arrows: sort navigation vs name scrolling, swapped by
/// `--arrow-keys-sort` exactly like upstream lines 281-288.
fn handle_horizontal(key: Key, ui: &mut UiState, opts: &mut RenderOpts, args: &Args) {
    let shifted = matches!(key, Key::ShiftLeft | Key::ShiftRight);
    let left = matches!(key, Key::Left | Key::ShiftLeft);
    // No flag: plain arrows scroll names, shift+arrows change sort;
    // `--arrow-keys-sort` swaps the two (upstream parity).
    if shifted == args.arrow_keys_sort {
        if left {
            ui.name_scroll = ui.name_scroll.saturating_sub(1);
        } else {
            ui.name_scroll = ui.name_scroll.saturating_add(1);
        }
        return;
    }
    let pos = SORT_CYCLE.iter().position(|k| *k == opts.sort_key).unwrap_or(0);
    let next = if left {
        pos.saturating_sub(1)
    } else {
        (pos + 1).min(SORT_CYCLE.len() - 1)
    };
    opts.sort_key = SORT_CYCLE[next].to_string();
}

/// Group toggle: hide the whole set when any member is visible,
// otherwise show it all (matches upstream disable/enable pairs).
pub(crate) fn toggle_group(ui: &mut UiState, group: &[&str]) {
    if group.iter().any(|n| !ui.is_hidden(n)) {
        for n in group {
            if !ui.is_hidden(n) {
                ui.toggle_hidden(n);
            }
        }
    } else {
        for n in group {
            ui.toggle_hidden(n);
        }
    }
}

/// PID under the cursor, via the same sorted rows the renderer shows.
/// Program-aggregate rows (`123+`) and headers resolve to `None`.
pub(crate) fn selected_pid(
    stats: &GlancesStats,
    opts: &RenderOpts,
    ui: &UiState,
) -> Option<u32> {
    let rows = process_rows(&stats.snapshot(), opts);
    rows.get(ui.selected).and_then(|r| r.pid.parse::<u32>().ok())
}
