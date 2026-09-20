//! Interactive terminal UI (default standalone mode on a TTY).
//!
//! Pure-stdlib curses-style interface: alternate screen, ANSI drawing,
//! poll-based keyboard input. Frame rendering is pure (`render.rs`,
//! unit-tested); this module owns the terminal guard, the input
//! thread, and the refresh loop.

pub mod actions;
pub mod hotkeys;
pub mod keys;
pub mod render;
pub mod term;

use std::io::Write;
use std::sync::mpsc;
use std::time::Duration;

use keys::{read_key, Key};
use render::{RenderOpts, UiState};
use term::RawMode;

use crate::cli::args::Args;
use crate::core::logger;
use crate::core::stats::GlancesStats;

/// What backs each refresh tick: local sampling or an SNMP agent.
pub enum UpdateDriver<'a> {
    Local,
    Snmp(&'a crate::core::snmp::SnmpCtx),
}

impl<'a> UpdateDriver<'a> {
    fn tick(&self, stats: &GlancesStats) -> Result<(), String> {
        match self {
            UpdateDriver::Local => stats.update().map_err(|e| e.to_string()),
            UpdateDriver::Snmp(ctx) => stats.update_snmp(ctx).map_err(|e| e.to_string()),
        }
    }
}

/// Run the interactive UI until quit, stop-after, or error. Restores the
/// terminal on every exit path via RAII guards.
pub fn run(stats: &GlancesStats, args: &Args) -> Result<(), String> {
    run_with(stats, args, &UpdateDriver::Local)
}

/// SNMP client-mode UI: same interface, ticks poll the agent
/// (upstream SNMP client curses parity).
pub fn run_snmp(
    stats: &GlancesStats,
    args: &Args,
    ctx: &crate::core::snmp::SnmpCtx,
) -> Result<(), String> {
    run_with(stats, args, &UpdateDriver::Snmp(ctx))
}

fn run_with(stats: &GlancesStats, args: &Args, driver: &UpdateDriver<'_>) -> Result<(), String> {
    let _raw = RawMode::enter().map_err(|e| format!("tui: raw mode: {}", e))?;
    let mut stdout = std::io::stdout();
    let enter = |out: &mut std::io::Stdout| -> std::io::Result<()> {
        out.write_all(term::ALT_ENTER.as_bytes())?;
        out.write_all(term::HIDE_CURSOR.as_bytes())?;
        out.flush()
    };
    let exit = |out: &mut std::io::Stdout| {
        let _ = out.write_all(term::SHOW_CURSOR.as_bytes());
        let _ = out.write_all(term::ALT_EXIT.as_bytes());
        let _ = out.flush();
    };
    if let Err(e) = enter(&mut stdout) {
        return Err(format!("tui: setup: {}", e));
    }

    // Non-blocking key pump: the input thread blocks on stdin, the loop
    // drains whatever arrived since the last tick.
    let (tx, rx) = mpsc::channel::<Key>();
    std::thread::spawn(move || {
        loop {
            match read_key(Duration::from_millis(120)) {
                Some(k) => {
                    if tx.send(k).is_err() {
                        break;
                    }
                    if k == Key::Quit {
                        break;
                    }
                }
                None => {}
            }
        }
    });

    let (mut rows, mut cols): (u16, u16);
    let mut opts = RenderOpts::from_args(args, 80);
    let mut ui = UiState::new(args.percpu);
    let mut tick: u32 = 0;
    let refresh = stats.refresh_time.max(0.1);

    loop {
        if let Err(e) = driver.tick(stats) {
            logger::warning(&format!("tui: refresh tick failed: {}", e));
        }
        if !args.export_targets.is_empty() {
            let keys = stats.plugin_keys();
            crate::exports::write_targets(&stats.snapshot(), args, &keys);
        }
        // Terminal resizes apply on the next frame.
        let (r, c) = term::size();
        rows = r;
        cols = c;
        opts.cols = cols as usize;
        // Drain all pending keys before rendering this frame.
        if drain_keys(&rx, &mut ui, &mut opts, stats, args) {
            break;
        }
        let frame = render::render(&stats.snapshot(), &opts, &ui, rows as usize);
        let _ = stdout.write_all(term::HOME.as_bytes());
        let _ = stdout.write_all(term::ERASE_DOWN.as_bytes());
        let _ = stdout.write_all(frame.as_bytes());
        let _ = stdout.flush();
        tick = tick.saturating_add(1);
        if let Some(max) = args.stop_after {
            if tick >= max {
                break;
            }
        }
        // Sleep in slices so keys land promptly even with long refresh
        // intervals; every slice drains before sleeping again.
        let slices = (refresh * 10.0).round().max(1.0) as u32;
        let mut quit = false;
        for _ in 0..slices {
            if drain_keys(&rx, &mut ui, &mut opts, stats, args) {
                quit = true;
                break;
            }
            // Manual refresh skips the remaining sleep slices.
            if std::mem::replace(&mut ui.refresh_now, false) {
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if quit {
            break;
        }
    }
    exit(&mut stdout);
    Ok(())
}

/// Drain pending keys through the hotkey dispatch. Returns true on quit.
fn drain_keys(
    rx: &mpsc::Receiver<Key>,
    ui: &mut UiState,
    opts: &mut RenderOpts,
    stats: &GlancesStats,
    args: &Args,
) -> bool {
    let mut quit = false;
    while let Ok(k) = rx.try_recv() {
        if actions::handle_key(k, ui, opts, stats, args) {
            quit = true;
        }
    }
    quit
}
