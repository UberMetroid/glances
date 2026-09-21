//! The hotkey byte table (upstream `_hotkeys` in `glances_curses.py`).
//!
//! Lowercase/digits set sort keys and headline toggles; uppercase and
//! symbols drive plugin switches plus the kill/nice/filter handlers.

use super::actions::{selected_pid, toggle_group, FULL_QUICKLOOK_OFF, PROCESS_GROUP, SIDEBAR, TOP_MENU};
use super::render::{ConfirmAction, RenderOpts, UiState};
use crate::core::logger;
use crate::core::stats::GlancesStats;

fn hide(ui: &mut UiState, name: &str) -> bool {
    ui.toggle_hidden(name);
    false
}

fn flip(flag: &mut bool) -> bool {
    *flag = !*flag;
    false
}

pub(crate) fn confirms(armed: ConfirmAction, b: u8) -> bool {
    matches!(
        (armed, b),
        (ConfirmAction::Kill(_), b'k')
            | (ConfirmAction::NiceUp(_), b'+')
            | (ConfirmAction::NiceDown(_), b'-')
    )
}

pub(crate) fn handle_byte(
    b: u8,
    ui: &mut UiState,
    opts: &mut RenderOpts,
    stats: &GlancesStats,
) -> bool {
    match b {
        b'q' => true,
        b'0' => {
            ui.irix_divide = !ui.irix_divide;
            if let Ok(mut guard) = stats.plugins.write() {
                for p in guard.iter_mut() {
                    p.set_irix_divide(ui.irix_divide);
                }
            }
            false
        }
        b'1' => flip(&mut ui.percpu),
        b'2' => {
            toggle_group(ui, SIDEBAR);
            false
        }
        b'3' => hide(ui, "quicklook"),
        b'4' => {
            toggle_group(ui, FULL_QUICKLOOK_OFF);
            false
        }
        b'5' => {
            toggle_group(ui, TOP_MENU);
            false
        }
        b'6' => flip(&mut opts.meangpu),
        b'7' => hide(ui, "npu"),
        b'8' => hide(ui, "mpp"),
        b'/' => flip(&mut opts.process_short_name),
        b'a' => sort_to(opts, "auto"),
        b'c' => sort_to(opts, "cpu_percent"),
        b'i' => sort_to(opts, "io_counters"),
        b'm' => sort_to(opts, "memory_percent"),
        b'o' => sort_to(opts, "cpu_num"),
        b'p' => sort_to(opts, "name"),
        b't' => sort_to(opts, "cpu_times"),
        b'u' => sort_to(opts, "username"),
        _ => handle_byte_upper(b, ui, opts, stats),
    }
}

fn sort_to(opts: &mut RenderOpts, key: &str) -> bool {
    opts.sort_key = key.to_string();
    false
}

/// Uppercase switches plus symbol handlers.
fn handle_byte_upper(
    b: u8,
    ui: &mut UiState,
    opts: &mut RenderOpts,
    stats: &GlancesStats,
) -> bool {
    match b {
        b'A' => hide(ui, "amps"),
        b'B' => flip(&mut opts.diskio_iops),
        b'C' => hide(ui, "cloud"),
        b'D' => hide(ui, "containers"),
        b'E' => clear_filter(stats),
        b'F' => flip(&mut opts.fs_free_space),
        b'G' => hide(ui, "gpu"),
        b'I' => hide(ui, "ip"),
        b'K' => hide(ui, "connections"),
        b'L' => flip(&mut opts.diskio_latency),
        b'M' => reset_minmax(stats),
        b'N' => hide(ui, "now"),
        b'P' => hide(ui, "ports"),
        b'Q' => enable_irq(ui, stats),
        b'R' => hide(ui, "raid"),
        b'S' => flip(&mut opts.sparkline),
        b'T' => flip(&mut opts.network_sum),
        b'U' => flip(&mut opts.network_cumul),
        b'V' => hide(ui, "vms"),
        b'W' => hide(ui, "wifi"),
        b'b' => flip(&mut opts.byte_units),
        b'd' => hide(ui, "diskio"),
        b'e' => pin_extended(ui, opts, stats),
        b'f' => { hide(ui, "fs"); hide(ui, "folders") }
        b'z' => { toggle_group(ui, PROCESS_GROUP); false }
        b'h' => flip(&mut ui.show_help),
        b'j' => flip(&mut opts.programs),
        b'k' => arm(ui, opts, stats, ConfirmAction::Kill(0)),
        b'l' => hide(ui, "alert"),
        b'n' => hide(ui, "network"),
        b'r' => hide(ui, "smart"),
        b's' => hide(ui, "sensors"),
        b'w' => clean_events(stats, false),
        b'x' => clean_events(stats, true),
        b'+' => arm(ui, opts, stats, ConfirmAction::NiceUp(0)),
        b'-' => arm(ui, opts, stats, ConfirmAction::NiceDown(0)),
        _ => false,
    }
}

/// Arm a kill/nice on the selected pid. The discriminant placeholder
/// is replaced with the real pid; no row under the cursor → ignore.
fn arm(ui: &mut UiState, opts: &RenderOpts, stats: &GlancesStats, kind: ConfirmAction) -> bool {
    ui.confirm = selected_pid(stats, opts, ui).map(|pid| match kind {
        ConfirmAction::Kill(_) => ConfirmAction::Kill(pid),
        ConfirmAction::NiceUp(_) => ConfirmAction::NiceUp(pid),
        ConfirmAction::NiceDown(_) => ConfirmAction::NiceDown(pid),
    });
    false
}

fn clear_filter(stats: &GlancesStats) -> bool {
    if let Ok(mut guard) = stats.plugins.write() {
        for p in guard.iter_mut() {
            p.set_process_filter(None);
        }
    }
    false
}

/// Upstream `reset_minmax_tag`: drop accumulated min/max/mean.
fn reset_minmax(stats: &GlancesStats) -> bool {
    if let Ok(mut guard) = stats.plugins.write() {
        for p in guard.iter_mut() {
            if let Some(m) = p.model_mut() {
                m.mmm_buffer.clear();
            }
        }
    }
    false
}

/// Upstream `enable_irq`: the irq plugin only exists when enabled —
/// register it on first use, then toggle display.
fn enable_irq(ui: &mut UiState, stats: &GlancesStats) -> bool {
    if stats.plugin_names().iter().any(|n| *n == "irq") {
        hide(ui, "irq")
    } else {
        crate::plugins::irq::register(stats);
        false
    }
}

fn clean_events(stats: &GlancesStats, critical: bool) -> bool {
    if let Ok(mut ev) = stats.events.lock() {
        ev.clean(critical);
    }
    false
}

/// Pin the selected pid as the extended process (also settable by
/// POST /api/4/processes/extended/{pid}); pressing again unpins.
fn pin_extended(ui: &UiState, opts: &RenderOpts, stats: &GlancesStats) -> bool {
    let pid = selected_pid(stats, opts, ui);
    if let Ok(mut guard) = stats.extended_process.lock() {
        let pinned = *guard;
        *guard = match (pinned, pid) {
            (Some(a), Some(b)) if a == b => None,
            (_, p) => p,
        };
    }
    false
}

pub(crate) fn execute_confirm(armed: ConfirmAction, opts: &RenderOpts, stats: &GlancesStats) {
    use crate::core::actions::{kill_pid, renice_pid};
    match armed {
        ConfirmAction::Kill(pid) => {
            // SIGTERM, like upstream's default kill signal.
            if let Err(e) = kill_pid(pid, 15) {
                logger::warning(&format!("kill {} failed: {}", pid, e));
            }
        }
        ConfirmAction::NiceUp(pid) | ConfirmAction::NiceDown(pid) => {
            // Nice is read live from the snapshot (never cached): `+`
            // lowers priority, `-` raises it, clamped to [-20, 19].
            let delta = if matches!(armed, ConfirmAction::NiceUp(_)) { 1 } else { -1 };
            match selected_nice(stats, opts, pid) {
                Some(cur) => {
                    let next = (cur + delta).clamp(-20, 19);
                    if let Err(e) = renice_pid(pid, next) {
                        logger::warning(&format!("renice {} failed: {}", pid, e));
                    }
                }
                None => logger::warning(&format!("process {} vanished before confirm", pid)),
            }
        }
    }
}

/// Current nice of a pid, read live from the snapshot.
fn selected_nice(stats: &GlancesStats, opts: &RenderOpts, pid: u32) -> Option<i32> {
    let snap = stats.snapshot();
    let arr = snap
        .as_object()?
        .get(if opts.programs { "programlist" } else { "processlist" })?
        .as_array()?;
    arr.iter().find_map(|v| {
        let o = v.as_object()?;
        if o.get("pid")?.as_f64()? as u32 != pid {
            return None;
        }
        o.get("nice")?.as_f64().map(|n| n as i32)
    })
}
