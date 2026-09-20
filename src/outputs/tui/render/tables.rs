//! Array-plugin tables and the process list.

use super::{fit, fmt_bytes, fmt_temp, section_head, RenderOpts, UiState};
use crate::core::filter::ProcessFilter;
use crate::core::value::Value;

/// Render an array plugin as a small table (union of scalar columns,
/// capped at `max_rows`). Used for network/diskio/fs/sensors and every
/// other list-shaped plugin.
pub(crate) fn render_array_table(
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
    if name == "fs" {
        // Upstream `fs` message shows exactly mount + Used/Free + Total.
        // `--fs-free-space` swaps Used for Free (`fs/__init__.py:298`).
        let middle = if opts.fs_free_space { "free" } else { "used" };
        for k in ["mnt_point", middle, "size"] {
            if rows.iter().any(|r| {
                r.as_object().is_some_and(|o| o.get(k).is_some_and(is_scalar))
            }) {
                cols.push(k.to_string());
            }
        }
    }
    if cols.is_empty() {
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

pub(crate) fn is_scalar(v: &Value) -> bool {
    matches!(v, Value::Null | Value::Bool(_) | Value::Int(_) | Value::Uint(_) | Value::Float(_) | Value::String(_))
}

fn is_temp_key(k: &str) -> bool {
    k.to_lowercase().contains("temp")
}

/// Format one table cell, converting rates/temps by key heuristics.
fn fmt_cell(v: &Value) -> String {
    fmt_scalar(v)
}

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

pub(crate) fn sort_proc_rows(rows: &mut [ProcRow], key: &str) {
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


