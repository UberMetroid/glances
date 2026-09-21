//! Generic array-plugin tables with per-plugin column specs.
//!
//! Most list plugins render as a union of scalar columns. Three
//! plugins pin upstream's curated columns instead: `fs` shows mount
//! + Used/Free + Total, `network` honors the cumul/sum toggles, and
//! `gpu` collapses to one mean row in meangpu mode.

use super::tables::fmt_scalar;
use super::{fit, fmt_temp, section_head, RenderOpts};
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
    // meangpu collapses the GPU list to one averaged row.
    let mean_row;
    let rows: &[Value] = if name == "gpu" && opts.meangpu {
        mean_row = vec![mean_gpu_row(rows)];
        &mean_row
    } else {
        rows
    };
    let mut out = section_head(opts, name);
    let mut cols: Vec<String> = match pinned_cols(name, opts) {
        Some(pinned) => pinned
            .into_iter()
            .filter(|k| rows.iter().any(|r| {
                r.as_object().is_some_and(|o| o.get(*k).is_some_and(is_scalar))
            }))
            .map(|k| k.to_string())
            .collect(),
        None => Vec::new(),
    };
    if cols.is_empty() {
        for row in rows.iter().take(8) {
            if let Some(obj) = row.as_object() {
                for (k, v) in obj {
                    if cols.len() >= 6 {
                        break;
                    }
                    if name == "network" && opts.network_cumul && k.ends_with("_rate_per_sec") {
                        continue;
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

/// Curated column sets. `None` keeps the generic scalar union.
fn pinned_cols(name: &str, opts: &RenderOpts) -> Option<Vec<&'static str>> {
    match name {
        // Upstream `fs` message: mount + Used/Free + Total.
        "fs" => Some(vec![
            "mnt_point",
            if opts.fs_free_space { "free" } else { "used" },
            "size",
        ]),
        // Upstream network sum view: one Rx+Tx column.
        "network" if opts.network_sum => Some(vec![
            "interface_name",
            if opts.network_cumul { "bytes_all_gauge" } else { "bytes_all_rate_per_sec" },
        ]),
        // Default network view: interface + live Rx/Tx rates (pinned so
        // rate-mechanic fields like gauges never crowd the union).
        "network" => Some(vec![
            "interface_name",
            "bytes_recv_rate_per_sec",
            "bytes_sent_rate_per_sec",
        ]),
        // GPU view: the upstream canonical columns.
        "gpu" => Some(vec![
            "name",
            "proc",
            "mem",
            "temperature",
        ]),
        // Upstream diskio iops/latency views (also fixes the dead
        // `--diskio-iops`/`--diskio-latency` startup flags).
        "diskio" if opts.diskio_latency => {
            Some(vec!["disk_name", "read_latency_ms", "write_latency_ms"])
        }
        "diskio" if opts.diskio_iops => {
            Some(vec!["disk_name", "read_count", "write_count"])
        }
        _ => None,
    }
}

/// Average `util_pct` across GPUs into one `mean` row.
fn mean_gpu_row(rows: &[Value]) -> Value {
    let utils: Vec<f64> = rows
        .iter()
        .filter_map(|r| r.as_object())
        .filter_map(|o| o.get("util_pct"))
        .filter_map(|v| v.as_f64())
        .collect();
    let avg = if utils.is_empty() {
        0.0
    } else {
        utils.iter().sum::<f64>() / utils.len() as f64
    };
    let mut m = std::collections::BTreeMap::new();
    m.insert("gpu_id".into(), Value::String("mean".into()));
    m.insert("util_pct".into(), Value::Float(avg));
    Value::Object(m)
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
