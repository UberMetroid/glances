//! Generic sections plus frame assembly (`render`) and help overlay.

use super::{fit, plugin_obj, section_head, RenderOpts, UiState};
use super::overview::{render_cpu, render_header, render_load, render_mem, render_quicklook};
use super::tables::{fmt_scalar, is_scalar, render_array_table, render_processes};
use crate::core::value::Value;

/// section (alert thresholds, cloud metadata, version banners…).
/// Generic one-line rendering for object plugins without a dedicated
/// section (thresholds, cloud metadata, version banners…).
fn render_generic_object(snap: &Value, opts: &RenderOpts, name: &str) -> Vec<String> {
    let obj = match plugin_obj(snap, name) {
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
