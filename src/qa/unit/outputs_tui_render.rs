//! Unit tests for the TUI renderer (pure snapshot → text, no TTY).

use std::collections::BTreeMap;

use crate::cli::args::Args;
use crate::core::value::Value;
use crate::outputs::tui::render::{fit, fmt_bytes, fmt_rate, fmt_temp, render, RenderOpts, UiState};

fn obj(pairs: &[(&str, Value)]) -> Value {
    let mut m = BTreeMap::new();
    for (k, v) in pairs {
        m.insert((*k).to_string(), v.clone());
    }
    Value::Object(m)
}

fn opts() -> RenderOpts {
    RenderOpts::from_args(&Args::default(), 100)
}

fn ui() -> UiState {
    UiState::new(false)
}

fn proc_entry(pid: u64, name: &str, cpu: f64) -> Value {
    obj(&[
        ("pid", Value::Uint(pid)),
        ("name", Value::String(name.into())),
        ("cmdline", Value::String(format!("/bin/{}", name))),
        ("username", Value::String("root".into())),
        ("num_threads", Value::Uint(1)),
        ("cpu_percent", Value::Float(cpu)),
        ("memory_percent", Value::Float(1.0)),
        (
            "memory_info",
            obj(&[("rss", Value::Uint(1024)), ("vms", Value::Uint(2048))]),
        ),
        ("status", Value::String("running".into())),
        ("nice", Value::Int(0)),
        (
            "cpu_times",
            obj(&[("user", Value::Uint(10)), ("system", Value::Uint(5))]),
        ),
        (
            "io_counters",
            obj(&[("read_bytes", Value::Uint(0)), ("write_bytes", Value::Uint(0))]),
        ),
        ("cpu_num", Value::Uint(0)),
    ])
}

fn snap_with_procs() -> Value {
    obj(&[
        ("system", obj(&[("hostname", Value::String("testbox".into()))])),
        ("cpu", obj(&[("total", Value::Float(42.0))])),
        (
            "processlist",
            Value::Array(vec![proc_entry(2, "b-proc", 5.0), proc_entry(1, "a-proc", 50.0)]),
        ),
    ])
}

#[test]
fn header_carries_host_and_version() {
    let frame = render(&snap_with_procs(), &opts(), &ui(), 40);
    assert!(frame.contains("testbox"), "frame:\n{}", frame);
    assert!(frame.contains("glances-rs"), "frame:\n{}", frame);
}

#[test]
fn empty_snapshot_still_renders_footer() {
    let frame = render(&Value::Object(BTreeMap::new()), &opts(), &ui(), 24);
    assert!(frame.contains("q quit"), "frame:\n{}", frame);
}

#[test]
fn process_table_sorts_and_selects() {
    let frame = render(&snap_with_procs(), &opts(), &ui(), 40);
    let a = frame.find("a-proc").expect("a-proc row");
    let b = frame.find("b-proc").expect("b-proc row");
    assert!(a < b, "cpu sort puts 50% before 5%");
    // Cursor on first row highlights it (inverse video).
    assert!(frame.contains("\x1b[7m"), "selected row must highlight");
}

#[test]
fn programs_mode_uses_programlist() {
    let snap = obj(&[(
        "programlist",
        Value::Array(vec![obj(&[
            ("name", Value::String("web".into())),
            ("username", Value::String("www".into())),
            ("nprocs", Value::Uint(3)),
            ("num_threads", Value::Uint(9)),
            ("cpu_percent", Value::Float(30.0)),
            ("memory_percent", Value::Float(4.0)),
            ("memory_info", obj(&[("rss", Value::Uint(100)), ("vms", Value::Uint(200))])),
            ("status", Value::String("running".into())),
            ("nice", Value::String("0".into())),
            ("cpu_times", obj(&[("user", Value::Uint(1)), ("system", Value::Uint(1))])),
            ("io_counters", obj(&[("read_bytes", Value::Uint(0)), ("write_bytes", Value::Uint(0))])),
            ("childrens", Value::Array(vec![Value::Uint(11)])),
        ])]),
    )]);
    let mut o = opts();
    o.programs = true;
    let frame = render(&snap, &o, &ui(), 40);
    assert!(frame.contains("PROGRAMS"), "frame:\n{}", frame);
    assert!(frame.contains("web"), "frame:\n{}", frame);
    assert!(frame.contains("11+"), "first child pid: {}", frame);
}

#[test]
fn kernel_threads_and_focus_filter() {
    let mut snap = snap_with_procs();
    if let Some(Value::Array(arr)) = snap
        .as_object_mut()
        .and_then(|o| o.get_mut("processlist"))
    {
        arr.push(obj(&[("name", Value::String("kworker".into()))]));
    }
    let mut o = opts();
    o.hide_kernel_threads = true;
    let frame = render(&snap, &o, &ui(), 40);
    assert!(!frame.contains("kworker"), "kthreads hidden: {}", frame);
    let mut o2 = opts();
    o2.focus = vec!["a-proc".to_string()];
    let frame2 = render(&snap, &o2, &ui(), 40);
    assert!(frame2.contains("a-proc") && !frame2.contains("b-proc"), "focus: {}", frame2);
}

#[test]
fn fahrenheit_and_ascii_fallbacks() {
    let o = opts();
    assert_eq!(fmt_temp(&o, 100.0), "100C");
    let mut f = opts();
    f.fahrenheit = true;
    assert_eq!(fmt_temp(&f, 100.0), "212F");
    let mut a = opts();
    a.style.unicode = false;
    let frame = render(&snap_with_procs(), &a, &ui(), 40);
    assert!(!frame.contains('█'), "ascii fallback must not emit blocks");
}

#[test]
fn units_format() {
    assert_eq!(fmt_bytes(512.0), "512B");
    assert_eq!(fmt_bytes(2048.0), "2.0KB");
    let o = opts();
    assert!(fmt_rate(&o, 125000.0).ends_with("b/s"), "bits by default");
    let mut b = opts();
    b.byte_units = true;
    assert!(fmt_rate(&b, 2048.0).contains("KB/s"), "bytes with --byte");
}

#[test]
fn sparkline_flag_swaps_bars() {
    let mut o = opts();
    o.sparkline = true;
    let frame = render(&snap_with_procs(), &o, &ui(), 40);
    assert!(
        frame.contains('▅') || frame.contains('█') || frame.contains('▄'),
        "spark: {}",
        frame
    );
}

#[test]
fn fs_free_space_swaps_used_for_free() {
    let entry = obj(&[
        ("mnt_point", Value::String("/".into())),
        ("device_name", Value::String("/dev/sda1".into())),
        ("fs_type", Value::String("ext4".into())),
        ("size", Value::Uint(100_000)),
        ("used", Value::Uint(40_000)),
        ("free", Value::Uint(60_000)),
        ("percent", Value::Float(40.0)),
    ]);
    let snap = obj(&[("fs", Value::Array(vec![entry]))]);
    let plain = render(&snap, &opts(), &ui(), 40);
    assert!(plain.contains("used"), "default shows Used:\n{}", plain);
    let mut o = opts();
    o.fs_free_space = true;
    let swapped = render(&snap, &o, &ui(), 40);
    assert!(!swapped.contains("used"), "flag hides Used:\n{}", swapped);
    assert!(swapped.contains("free"), "flag shows Free:\n{}", swapped);
}

#[test]
fn fit_counts_visible_width_only() {
    let styled = "\x1b[1mhello\x1b[0m world";
    assert_eq!(fit(styled, 5), "\x1b[1mhello\x1b[0m");
    assert_eq!(fit("abcdef", 3), "abc");
}
