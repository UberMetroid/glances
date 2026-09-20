//! Dispatch tests for the TUI hotkey table (upstream `_hotkeys`
//! parity): every key lands in the right state, groups toggle as a
//! unit, and kill/nice arm + disarm without acting.

use crate::cli::args::Args;
use crate::core::events::Event;
use crate::core::stats::GlancesStats;
use crate::core::threshold::Severity;
use crate::outputs::tui::actions::handle_key;
use crate::outputs::tui::keys::Key;
use crate::outputs::tui::render::{ConfirmAction, RenderOpts, UiState};
use crate::plugins;

fn harness() -> (GlancesStats, Args, RenderOpts, UiState) {
    let stats = GlancesStats::new(2.0);
    plugins::register_all(&stats);
    let args = Args::default();
    let opts = RenderOpts::from_args(&args, 100);
    (stats, args, opts, UiState::new(false))
}

fn byte(b: u8) -> Key {
    Key::Byte(b)
}

#[test]
fn quit_paths() {
    let (stats, args, mut opts, mut ui) = harness();
    assert!(handle_key(byte(b'q'), &mut ui, &mut opts, &stats, &args));
    assert!(handle_key(Key::Quit, &mut ui, &mut opts, &stats, &args));
    assert!(handle_key(Key::Interrupt, &mut ui, &mut opts, &stats, &args));
}

#[test]
fn headline_toggles() {
    let (stats, args, mut opts, mut ui) = harness();
    assert!(!handle_key(byte(b'1'), &mut ui, &mut opts, &stats, &args));
    assert!(ui.percpu);
    assert!(!handle_key(byte(b'h'), &mut ui, &mut opts, &stats, &args));
    assert!(ui.show_help);
    assert!(!handle_key(byte(b'3'), &mut ui, &mut opts, &stats, &args));
    assert!(ui.is_hidden("quicklook"));
    assert!(!handle_key(byte(b'F'), &mut ui, &mut opts, &stats, &args));
    assert!(opts.fs_free_space);
    assert!(!handle_key(byte(b'6'), &mut ui, &mut opts, &stats, &args));
    assert!(opts.meangpu);
}

#[test]
fn top_menu_group_round_trip() {
    let (stats, args, mut opts, mut ui) = harness();
    handle_key(byte(b'5'), &mut ui, &mut opts, &stats, &args);
    for p in ["quicklook", "cpu", "mem", "memswap", "load"] {
        assert!(ui.is_hidden(p), "{p} hidden by -5");
    }
    // Second press restores the whole group.
    handle_key(byte(b'5'), &mut ui, &mut opts, &stats, &args);
    for p in ["quicklook", "cpu", "mem", "memswap", "load"] {
        assert!(!ui.is_hidden(p), "{p} restored");
    }
}

#[test]
fn sort_keys_and_cycle() {
    let (stats, args, mut opts, mut ui) = harness();
    handle_key(byte(b'm'), &mut ui, &mut opts, &stats, &args);
    assert_eq!(opts.sort_key, "memory_percent");
    // Plain arrows scroll names by default...
    handle_key(Key::Right, &mut ui, &mut opts, &stats, &args);
    assert_eq!(ui.name_scroll, 1);
    assert_eq!(opts.sort_key, "memory_percent");
    // ...shift+arrows cycle the sort field.
    handle_key(Key::ShiftRight, &mut ui, &mut opts, &stats, &args);
    assert_eq!(opts.sort_key, "name");
    // With --arrow-keys-sort the roles swap.
    let mut sort_args = Args::default();
    sort_args.arrow_keys_sort = true;
    handle_key(Key::Right, &mut ui, &mut opts, &stats, &sort_args);
    assert_eq!(opts.sort_key, "cpu_times");
}

#[test]
fn filter_edit_apply_and_cancel() {
    let (stats, args, mut opts, mut ui) = harness();
    handle_key(Key::Enter, &mut ui, &mut opts, &stats, &args);
    assert_eq!(ui.filter_input, Some(String::new()));
    handle_key(byte(b'x'), &mut ui, &mut opts, &stats, &args);
    assert_eq!(ui.filter_input, Some("x".into()));
    // `q` is text while editing, not quit.
    assert!(!handle_key(byte(b'q'), &mut ui, &mut opts, &stats, &args));
    assert_eq!(ui.filter_input, Some("xq".into()));
    handle_key(Key::Enter, &mut ui, &mut opts, &stats, &args);
    assert_eq!(ui.filter_input, None);
    // Reopen and cancel with ESC.
    handle_key(Key::Enter, &mut ui, &mut opts, &stats, &args);
    assert!(!handle_key(Key::Quit, &mut ui, &mut opts, &stats, &args));
    assert_eq!(ui.filter_input, None);
}

#[test]
fn kill_arms_and_wrong_key_disarms() {
    let (stats, args, mut opts, mut ui) = harness();
    let _ = stats.update();
    handle_key(byte(b'k'), &mut ui, &mut opts, &stats, &args);
    assert!(matches!(ui.confirm, Some(ConfirmAction::Kill(_))));
    // Wrong key disarms with no kill attempted (and does nothing else).
    assert!(!handle_key(byte(b'z'), &mut ui, &mut opts, &stats, &args));
    assert_eq!(ui.confirm, None);
    assert!(!ui.is_hidden("processlist"));
    // Now disarmed, `z` hides the process group.
    handle_key(byte(b'z'), &mut ui, &mut opts, &stats, &args);
    assert!(ui.is_hidden("processlist"));
}

#[test]
fn clean_log_keys() {
    let (stats, args, mut opts, mut ui) = harness();
    let push = |sev| {
        stats.events.lock().unwrap().push(Event {
            severity: sev,
            stat: "cpu".into(),
            value: 99.0,
            timestamp: std::time::SystemTime::now(),
        });
    };
    push(Severity::Warning);
    push(Severity::Critical);
    handle_key(byte(b'w'), &mut ui, &mut opts, &stats, &args);
    let kept: Vec<Severity> =
        stats.events.lock().unwrap().snapshot().iter().map(|e| e.severity).collect();
    assert_eq!(kept, vec![Severity::Critical]);
    handle_key(byte(b'x'), &mut ui, &mut opts, &stats, &args);
    assert!(stats.events.lock().unwrap().is_empty());
}

#[test]
fn irq_registers_on_first_use() {
    let (stats, args, mut opts, mut ui) = harness();
    assert!(!stats.plugin_names().iter().any(|n| *n == "irq"));
    handle_key(byte(b'Q'), &mut ui, &mut opts, &stats, &args);
    assert!(stats.plugin_names().iter().any(|n| *n == "irq"));
}

#[test]
fn irix_zero_key() {
    let (stats, args, mut opts, mut ui) = harness();
    handle_key(byte(b'0'), &mut ui, &mut opts, &stats, &args);
    assert!(ui.irix_divide);
}
