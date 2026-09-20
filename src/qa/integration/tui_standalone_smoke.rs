//! End-to-end smoke for the M15a standalone TUI scaffolding.
//!
//! Registers every built-in plugin, drives a single render pass over
//! the initial (un-updated) stats, and verifies the output contains
//! the expected per-plugin substrings. No live `/proc` reads required —
//! M15a scaffolding only proves the renderer wires correctly into
//! `GlancesStats`. The same assertions run cross-platform because the
//! plugins default to zero-valued stats when `update()` is not called.

use crate::core::plugin::Plugin;
use crate::core::stats::GlancesStats;
use crate::outputs::tui::render;

/// Render every registered plugin's stats into a single buffer and
/// return it. Plugins unknown to the renderer are silently skipped —
/// M15b widens the dispatch.
fn render_all(stats: &GlancesStats) -> String {
    let mut buf = String::new();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    for p in guard.iter() {
        match p.name() {
            "cpu" => render::render_cpu(p.as_ref(), &mut buf),
            "mem" => render::render_mem(p.as_ref(), &mut buf),
            "load" => render::render_load(p.as_ref(), &mut buf),
            "network" => render::render_network(p.as_ref(), &mut buf),
            "processcount" => render::render_processes(p.as_ref(), &mut buf),
            _ => {}
        }
    }
    buf
}

#[test]
fn register_all_and_render_each_plugin() {
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    let names = stats.plugin_names();
    // Register_all wires every plugin in the order Python Glances uses.
    assert!(names.contains(&"cpu"));
    assert!(names.contains(&"mem"));
    assert!(names.contains(&"load"));
    assert!(names.contains(&"network"));
    assert!(names.contains(&"processcount"));

    let buf = render_all(&stats);
    assert!(buf.contains("[cpu]"), "missing cpu section: {}", &buf[..buf.len().min(80)]);
    assert!(buf.contains("[mem]"), "missing mem section");
    assert!(buf.contains("[load]"), "missing load section");
    assert!(buf.contains("[network]"), "missing network section");
    assert!(buf.contains("[processes]"), "missing processes section");
}

#[test]
fn cpu_section_includes_total_field() {
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    let mut buf = String::new();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let cpu = guard.iter().find(|p| p.name() == "cpu").expect("cpu plugin registered");
    render::render_cpu(cpu.as_ref(), &mut buf);
    assert!(buf.contains("total="), "cpu line should contain total=: {}", buf);
    assert!(buf.contains("user="));
    assert!(buf.contains("idle="));
}

#[test]
fn mem_section_includes_percent() {
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    let mut buf = String::new();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let mem = guard.iter().find(|p| p.name() == "mem").expect("mem plugin registered");
    render::render_mem(mem.as_ref(), &mut buf);
    assert!(buf.contains("pct="), "mem line should contain pct=: {}", buf);
    assert!(buf.contains("total="));
    assert!(buf.contains("used="));
}

#[test]
fn load_section_includes_three_averages() {
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    let mut buf = String::new();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let load = guard.iter().find(|p| p.name() == "load").expect("load plugin registered");
    render::render_load(load.as_ref(), &mut buf);
    assert!(buf.contains("1m="), "load line should contain 1m=: {}", buf);
    assert!(buf.contains("5m="));
    assert!(buf.contains("15m="));
}

#[test]
fn network_section_emits_nic_count() {
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    let mut buf = String::new();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let net = guard.iter().find(|p| p.name() == "network").expect("network plugin registered");
    render::render_network(net.as_ref(), &mut buf);
    assert!(buf.contains("nics="), "network line should contain nics=: {}", buf);
    assert!(buf.contains("rx="));
    assert!(buf.contains("tx="));
}

#[test]
fn processes_section_emits_count() {
    let stats = GlancesStats::new(2.0);
    crate::plugins::register_all(&stats);
    let mut buf = String::new();
    let guard = stats.plugins.read().unwrap_or_else(|e| e.into_inner());
    let pc = guard.iter().find(|p| p.name() == "processcount").expect("processcount plugin registered");
    render::render_processes(pc.as_ref(), &mut buf);
    assert!(buf.contains("count="), "processes line should contain count=: {}", buf);
}

#[test]
fn process_table_stub_emits_marker() {
    let mut buf = String::new();
    crate::outputs::tui::process_table::render(&mut buf);
    assert!(buf.contains("process list"));
    assert!(buf.contains("M15"));
}

#[test]
fn alert_bar_stub_emits_blank_line() {
    let mut buf = String::from("HEAD");
    crate::outputs::tui::alert_bar::render(&mut buf);
    assert_eq!(buf, "HEAD\n");
}

#[test]
fn sparkline_helper_handles_constants_and_trend() {
    let flat = crate::outputs::tui::sparkline::render_sparkline(&[5.0, 5.0, 5.0], 10);
    assert_eq!(flat.chars().count(), 3);
    assert!(flat.chars().all(|c| c == '▁'));
    let rising = crate::outputs::tui::sparkline::render_sparkline(&[0.0, 5.0, 10.0], 10);
    assert_eq!(rising.chars().count(), 3);
    assert!(rising.starts_with('▁'));
    assert!(rising.ends_with('█'));
}

#[test]
fn key_handler_returns_refresh_by_default() {
    let stop = std::sync::atomic::AtomicBool::new(false);
    let action = crate::outputs::tui::key::next_action(&stop);
    assert_eq!(action, crate::outputs::tui::key::Action::Refresh);
}
