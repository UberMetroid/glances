//! Process list rows, sorting, and scalar formatting.

use super::{fit, fmt_bytes, section_head, RenderOpts, UiState};
use crate::core::filter::ProcessFilter;
use crate::core::value::Value;

pub(crate) fn fmt_scalar(v: &Value) -> String {
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

pub(crate) struct ProcRow {
    pub pid: String,
    user: String,
    nice: String,
    cpu: f64,
    mem: f64,
    cpu_times: f64,
    io_bytes: f64,
    cpu_num: u64,
    rss: String,
    name: String,
    cmdline: String,
}

/// Collect, filter, and sort process rows. `--programs` switches to the
/// program-aggregated list; focus terms filter by regex on name/cmdline.
pub(crate) fn process_rows(snap: &Value, opts: &RenderOpts) -> Vec<ProcRow> {
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
        // `cmdline` is an argv array (processlist, upstream parity) or a
        // joined string (programlist) — normalize to display text.
        let cmdline = match get("cmdline") {
            Some(v) if v.as_str().is_some() => v.as_str().unwrap_or("").to_string(),
            Some(v) => v
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|e| e.as_str())
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .unwrap_or_default(),
            None => String::new(),
        };
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
        let times = get("cpu_times").and_then(|v| v.as_object());
        let time_sum = ["user", "system"]
            .iter()
            .filter_map(|k| times.and_then(|t| t.get(*k)).and_then(|v| v.as_f64()))
            .sum();
        let io = get("io_counters").and_then(|v| v.as_object());
        let io_sum = ["read_bytes", "write_bytes"]
            .iter()
            .filter_map(|k| io.and_then(|t| t.get(*k)).and_then(|v| v.as_f64()))
            .sum();
        rows.push(ProcRow {
            pid,
            user: get("username").and_then(|v| v.as_str()).unwrap_or("?").to_string(),
            nice: get("nice").map(fmt_scalar).unwrap_or_else(|| "-".to_string()),
            cpu,
            mem,
            cpu_times: time_sum,
            io_bytes: io_sum,
            cpu_num: get("cpu_num").and_then(|v| v.as_f64()).unwrap_or(0.0) as u64,
            rss: fmt_bytes(rss),
            name,
            cmdline,
        });
    }
    sort_proc_rows(&mut rows, &opts.sort_key);
    rows
}

pub(crate) fn sort_proc_rows(rows: &mut [ProcRow], key: &str) {
    use std::cmp::Ordering::Equal;
    let desc = |a: f64, b: f64| b.partial_cmp(&a).unwrap_or(Equal);
    match key {
        "name" => rows.sort_by(|a, b| a.name.cmp(&b.name)),
        "pid" => rows.sort_by(|a, b| a.pid.cmp(&b.pid)),
        "username" | "user" => rows.sort_by(|a, b| a.user.cmp(&b.user)),
        "memory_percent" | "mem" => rows.sort_by(|a, b| desc(a.mem, b.mem)),
        "cpu_times" | "time" => rows.sort_by(|a, b| desc(a.cpu_times, b.cpu_times)),
        "io_counters" | "io" => rows.sort_by(|a, b| desc(a.io_bytes, b.io_bytes)),
        "cpu_num" => rows.sort_by(|a, b| a.cpu_num.cmp(&b.cpu_num)),
        // `auto` and anything unknown fall back to CPU (upstream
        // auto mode starts on CPU and only switches on extremes).
        _ => rows.sort_by(|a, b| desc(a.cpu, b.cpu)),
    }
}

pub(crate) fn render_processes(snap: &Value, opts: &RenderOpts, ui: &UiState, max_rows: usize) -> Vec<String> {
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


