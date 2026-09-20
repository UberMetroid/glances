//! Snapshot → styled screen text for the TUI.
//!
//! Every registered plugin appears: headline plugins (system, cpu,
//! mem, load, network, disk I/O, filesystems, sensors, processes) get
//! bars and tables; the rest render as compact generic sections. All
//! drawing honors the CLI display toggles (`Style`) so `--disable-*`
//! flags behave the same as upstream curses options.
//!
//! Pure functions over `Value` snapshots — no TTY needed, fully unit
//! tested. The live loop lives in `super` (`mod.rs`).

use std::collections::BTreeMap;

use super::term::{bar, pct_color, spark, Style};
use crate::cli::args::Args;
use crate::core::filter::ProcessFilter;
use crate::core::value::Value;

/// Frame options: toggles + terminal size.
#[derive(Debug, Clone)]
pub struct RenderOpts {
    pub style: Style,
    pub cols: usize,
    pub fahrenheit: bool,
    pub byte_units: bool,
    pub percpu: bool,
    pub programs: bool,
    pub sort_key: String,
    pub hide_kernel_threads: bool,
    pub process_short_name: bool,
    pub hide_public_info: bool,
    pub focus: Vec<String>,
    pub separator: bool,
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

fn plugin<'a>(snap: &'a Value, name: &str) -> Option<&'a BTreeMap<String, Value>> {
    snap.as_object()?.get(name)?.as_object()
}

fn num(snap: &Value, plugin: &str, key: &str) -> f64 {
    plugin(snap, plugin)
        .and_then(|o| o.get(key))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}

fn text(snap: &Value, plugin: &str, key: &str) -> String {
    match plugin(snap, plugin).and_then(|o| o.get(key)) {
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

fn title_bar(style: &Style, cols: usize, name: &str) -> String {
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

fn pct_styled(style: &Style, pct: f64) -> String {
    style.fg(pct_color(pct), &format!("{:5.1}%", pct.clamp(0.0, 100.0)))
}

fn section_head(opts: &RenderOpts, name: &str) -> Vec<String> {
    if opts.separator {
        vec![title_bar(&opts.style, opts.cols, name)]
    } else {
        vec![opts.style.b(&name.to_uppercase())]
    }
}

fn render_header(snap: &Value, opts: &RenderOpts) -> Vec<String> {
    let host = text(snap, "system", "hostname");
    let os = text(snap, "system", "os_name");
    let upt = num(snap, "uptime", "seconds") as u64;
    let (h, m, s) = (upt / 3600, upt / 60 % 60, upt % 60);
    vec![fit(
        &format!("glances-rs {}  {}  {}  up {:02}:{:02}:{:02}", env!("CARGO_PKG_VERSION"), host, os, h, m, s),
        opts.cols,
    )]
}

fn render_quicklook(snap: &Value, opts: &RenderOpts) -> Vec<String> {
    let cpu = num(snap, "cpu", "total");
    let mem = num(snap, "mem", "percent");
    let swap = num(snap, "memswap", "percent");
    let load = num(snap, "load", "min1");
    let w = (opts.cols.saturating_sub(48) / 3).max(8);
    // --sparkline swaps bars for single-cell spark blocks.
    let cpu_viz = if opts.sparkline { spark(&opts.style, cpu) } else { bar(&opts.style, cpu, w) };
    let mem_viz = if opts.sparkline { spark(&opts.style, mem) } else { bar(&opts.style, mem, w) };
    vec![fit(
        &format!(
            "CPU {} {}  MEM {} {}  SWAP {:5.1}%  LOAD {:.2}",
            pct_styled(&opts.style, cpu),
            cpu_viz,
            pct_styled(&opts.style, mem),
            mem_viz,
            swap.clamp(0.0, 100.0),
            load
        ),
        opts.cols,
    )]
}

fn render_cpu(snap: &Value, opts: &RenderOpts, ui: &UiState) -> Vec<String> {
    let mut out = section_head(opts, "cpu");
    if let Some(obj) = plugin(snap, "cpu") {
        let total = obj.get("total").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let user = obj.get("user").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let system = obj.get("system").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let w = opts.cols.saturating_sub(30).max(10);
        out.push(fit(
            &format!(
                "total {} {}  user {:.1} sys {:.1}",
                pct_styled(&opts.style, total),
                bar(&opts.style, total, w),
                user,
                system
            ),
            opts.cols,
        ));
    }
    if ui.percpu {
        let cores = snap.as_object().and_then(|o| o.get("percpu")).and_then(|v| v.as_array());
        if let Some(cores) = cores {
            for (i, c) in cores.iter().enumerate() {
                let v = c
                    .as_f64()
                    .or_else(|| c.as_object().and_then(|o| o.get("total")).and_then(|t| t.as_f64()))
                    .unwrap_or(0.0);
                out.push(fit(
                    &format!("cpu{:<3} {} {}", i, pct_styled(&opts.style, v), bar(&opts.style, v, 12)),
                    opts.cols,
                ));
            }
        }
    }
    out
}

fn render_mem(snap: &Value, opts: &RenderOpts) -> Vec<String> {
    let mut out = section_head(opts, "mem");
    if plugin(snap, "mem").is_some() {
        let pct = num(snap, "mem", "percent");
        let used = num(snap, "mem", "used");
        let total = num(snap, "mem", "total");
        let w = opts.cols.saturating_sub(40).max(10);
        out.push(fit(
            &format!(
                "RAM {} {}  {}/{}",
                pct_styled(&opts.style, pct),
                bar(&opts.style, pct, w),
                fmt_bytes(used),
                fmt_bytes(total)
            ),
            opts.cols,
        ));
    }
    if plugin(snap, "memswap").is_some() {
        let pct = num(snap, "memswap", "percent");
        out.push(fit(&format!("SWP {}", pct_styled(&opts.style, pct)), opts.cols));
    }
    out
}

fn render_load(snap: &Value, opts: &RenderOpts) -> Vec<String> {
    let mut out = section_head(opts, "load");
    if plugin(snap, "load").is_some() {
        out.push(fit(
            &format!(
                "1m {:.2}  5m {:.2}  15m {:.2}  cores {}",
                num(snap, "load", "min1"),
                num(snap, "load", "min5"),
                num(snap, "load", "min15"),
                num(snap, "load", "cpucore") as u64
            ),
            opts.cols,
        ));
    }
    out
}

/// Render an array plugin as a small table (union of scalar columns,
/// capped at `max_rows`). Used for network/diskio/fs/sensors and every
/// other list-shaped plugin.
fn render_array_table(
    snap: &Value,
    opts: &RenderOpts,
    name: &str,
    max_rows: usize,
) -> Vec<String> {
    let rows = match snap.as_object().and_then(|o| o.get(name)).and_then(|v| v.as_array()) {
        Some(r) => r,
        None => return Vec::new(),
    };
    if rows.is_empty() {
        return Vec::new();
    }
    let mut out = section_head(opts, name);
    let mut cols: Vec<String> = Vec::new();
    for row in rows.iter().take(8) {
        if let Some(obj) = row.as_object() {
            for (k, v) in obj {
                if cols.len() >= 6 {
                    break;
                }
                if !cols.contains(k) && is_scalar(v) {
                    cols.push(k.clone());
                }
            }
        }
    }
    if cols.is_empty() {
        return out;
    }
    out.push(fit(&format!("  {}", cols.join(" ")), opts.cols));
    for row in rows.iter().take(max_rows) {
        let obj = match row.as_object() {
            Some(o) => o,
            None => continue,
        };
        let cells: Vec<String> = cols
            .iter()
            .map(|k| {
                let mut s = obj.get(k).map(fmt_cell).unwrap_or_else(|| "-".to_string());
                if opts.fahrenheit && is_temp_key(k) {
                    if let Some(v) = obj.get(k).and_then(|v| v.as_f64()) {
                        s = fmt_temp(opts, v);
                    }
                }
                if opts.hide_public_info && k.to_lowercase().contains("public") {
                    s = "hidden".to_string();
                }
                s
            })
            .collect();
        out.push(fit(&format!("  {}", cells.join(" ")), opts.cols));
    }
    out
}

fn is_scalar(v: &Value) -> bool {
    matches!(v, Value::Null | Value::Bool(_) | Value::Int(_) | Value::Uint(_) | Value::Float(_) | Value::String(_))
}

fn is_temp_key(k: &str) -> bool {
    k.to_lowercase().contains("temp")
}

/// Format one table cell, converting rates/temps by key heuristics.
fn fmt_cell(v: &Value) -> String {
    fmt_scalar(v)
}

fn fmt_scalar(v: &Value) -> String {
    match v {
        Value::Null => "-".to_string(),
        Value::Bool(true) => "y".to_string(),
        Value::Bool(false) => "n".to_string(),
        Value::Int(i) => format!("{}", i),
        Value::Uint(u) => format!("{}", u),
        Value::Float(f) => {
            if f.fract() == 0.0 && f.abs() < 1e12 {
                format!("{}", *f as i64)
            } else {
                format!("{:.1}", f)
            }
        }
        Value::String(s) => s.clone(),
        Value::Object(_) | Value::Array(_) => "…".to_string(),
    }
}

struct ProcRow {
    pid: String,
    user: String,
    nice: String,
    cpu: f64,
    mem: f64,
    rss: String,
    name: String,
    cmdline: String,
}

/// Collect, filter, and sort process rows. `--programs` switches to the
/// program-aggregated list; focus terms filter by regex on name/cmdline.
fn process_rows(snap: &Value, opts: &RenderOpts) -> Vec<ProcRow> {
    let plugin_name = if opts.programs { "programlist" } else { "processlist" };
    let arr = match snap
        .as_object()
        .and_then(|o| o.get(plugin_name))
        .and_then(|v| v.as_array())
    {
        Some(a) => a,
        None => return Vec::new(),
    };
    let filters: Vec<ProcessFilter> = opts
        .focus
        .iter()
        .filter_map(|pat| ProcessFilter::new(pat).ok())
        .filter(|f| f.is_active())
        .collect();
    let mut rows = Vec::new();
    for item in arr {
        let o = match item.as_object() {
            Some(o) => o,
            None => continue,
        };
        let get = |k: &str| o.get(k);
        let name = get("name").and_then(|v| v.as_str()).unwrap_or("?").to_string();
        let cmdline = get("cmdline").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if opts.hide_kernel_threads && cmdline.is_empty() {
            continue;
        }
        if !filters.is_empty()
            && !filters.iter().any(|f| f.matches(&name, &cmdline))
        {
            continue;
        }
        let cpu = get("cpu_percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let mem = get("memory_percent").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let rss = get("memory_info")
            .and_then(|v| v.as_object())
            .and_then(|m| m.get("rss"))
            .and_then(|v| v.as_f64())
            .unwrap_or(0.0);
        let pid = match get("pid").and_then(|v| v.as_f64()) {
            Some(p) => format!("{}", p as u64),
            None => get("childrens")
                .and_then(|v| v.as_array())
                .and_then(|a| a.first())
                .and_then(|v| v.as_f64())
                .map(|p| format!("{}+", p as u64))
                .unwrap_or_else(|| "-".to_string()),
        };
        rows.push(ProcRow {
            pid,
            user: get("username").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
            nice: get("nice").map(fmt_scalar).unwrap_or_else(|| "-".to_string()),
            cpu,
            mem,
            rss: fmt_bytes(rss),
            name,
            cmdline,
        });
    }
    sort_proc_rows(&mut rows, &opts.sort_key);
    rows
}

fn sort_proc_rows(rows: &mut [ProcRow], key: &str) {
    match key {
        "name" => rows.sort_by(|a, b| a.name.cmp(&b.name)),
        "pid" => rows.sort_by(|a, b| a.pid.cmp(&b.pid)),
        "username" | "user" => rows.sort_by(|a, b| a.user.cmp(&b.user)),
        "memory_percent" | "mem" => rows.sort_by(|a, b| {
            b.mem.partial_cmp(&a.mem).unwrap_or(std::cmp::Ordering::Equal)
        }),
        _ => rows.sort_by(|a, b| {
            b.cpu.partial_cmp(&a.cpu).unwrap_or(std::cmp::Ordering::Equal)
        }),
    }
}

fn render_processes(snap: &Value, opts: &RenderOpts, ui: &UiState, max_rows: usize) -> Vec<String> {
    let title = if opts.programs { "programs" } else { "processes" };
    let mut out = section_head(opts, title);
    let rows = process_rows(snap, opts);
    if rows.is_empty() {
        return out;
    }
    out.push(fit("  PID USER     NI %CPU %MEM RSS      NAME", opts.cols));
    let sel = ui.selected.min(rows.len().saturating_sub(1));
    for (i, r) in rows.iter().take(max_rows).enumerate() {
        let label = if opts.process_short_name || r.cmdline.is_empty() {
            r.name.clone()
        } else {
            r.cmdline.clone()
        };
        let line = format!(
            "  {:>5} {:<8} {:>2} {:>4.1} {:>4.1} {:>7} {}",
            r.pid, r.user, r.nice, r.cpu, r.mem, r.rss, label
        );
        let line = fit(&line, opts.cols);
        if i == sel {
            out.push(opts.style.inv(&line));
        } else {
            out.push(line);
        }
    }
    out
}

/// Generic one-line rendering for object plugins without a dedicated
/// section (alert thresholds, cloud metadata, version banners…).
fn render_generic_object(snap: &Value, opts: &RenderOpts, name: &str) -> Vec<String> {
    let obj = match plugin(snap, name) {
        Some(o) => o,
        None => return Vec::new(),
    };
    let parts: Vec<String> = obj
        .iter()
        .filter(|(_, v)| is_scalar(v))
        .take(8)
        .map(|(k, v)| format!("{}: {}", k, fmt_scalar(v)))
        .collect();
    if parts.is_empty() {
        return Vec::new();
    }
    let mut out = section_head(opts, name);
    out.push(fit(&format!("  {}", parts.join("  ")), opts.cols));
    out
}

/// Plugins with dedicated sections above; everything else renders generic.
const CURATED: &[&str] = &[
    "system", "uptime", "now", "quicklook", "cpu", "percpu", "mem", "memswap", "load",
];

/// Assemble one full frame. `rows` caps total lines (footer included).
pub fn render(snap: &Value, opts: &RenderOpts, ui: &UiState, rows: usize) -> String {
    if ui.show_help {
        return render_help(opts);
    }
    let mut lines = render_header(snap, opts);
    lines.extend(render_quicklook(snap, opts));
    lines.extend(render_cpu(snap, opts, ui));
    lines.extend(render_mem(snap, opts));
    lines.extend(render_load(snap, opts));
    // Array plugins with useful tables first, in a stable order.
    for name in ["network", "diskio", "fs", "sensors", "connections", "ports", "containers"] {
        lines.extend(render_array_table(snap, opts, name, 6));
    }
    // The process table gets whatever space remains (min 4 rows).
    let reserved = lines.len() + 2;
    let proc_rows = rows.saturating_sub(reserved).max(4).min(30);
    lines.extend(render_processes(snap, opts, ui, proc_rows));
    // Everything else, generic.
    if let Some(top) = snap.as_object() {
        let mut names: Vec<&str> = top
            .keys()
            .map(|k| k.as_str())
            .filter(|k| !CURATED.contains(k))
            .filter(|k| !["network", "diskio", "fs", "sensors", "connections", "ports", "containers", "processlist", "programlist"].contains(k))
            .collect();
        names.sort_unstable();
        for name in names {
            if lines.len() + 3 >= rows {
                break;
            }
            match top.get(name) {
                Some(Value::Array(_)) => {
                    lines.extend(render_array_table(snap, opts, name, 4))
                }
                Some(Value::Object(_)) => lines.extend(render_generic_object(snap, opts, name)),
                _ => {}
            }
        }
    }
    lines.push(fit("q quit · h help · ↑↓ select · 1 per-cpu", opts.cols));
    // Cap to the terminal height, then reset attributes per line so a
    // truncated styled line can't bleed into the next one.
    lines.truncate(rows.max(1));
    lines.into_iter().map(|l| format!("{}\x1b[0m", l)).collect::<Vec<_>>().join("\r\n")
}

fn render_help(opts: &RenderOpts) -> String {
    let mut lines = vec![opts.style.b("glances-rs keys")];
    for (k, desc) in [
        ("q / ESC", "quit"),
        ("↑ / ↓", "move process cursor"),
        ("1", "toggle per-CPU view"),
        ("h", "toggle this help"),
    ] {
        lines.push(format!("  {:<10} {}", k, desc));
    }
    lines.push(String::new());
    lines.push("Sorting, filters, and display toggles are CLI flags;".to_string());
    lines.push("see --help for --sort-processes, -f, --programs.".to_string());
    lines.into_iter().map(|l| fit(&l, opts.cols)).collect::<Vec<_>>().join("\r\n")
}
