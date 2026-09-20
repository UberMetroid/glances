//! Headline sections: header, quicklook, CPU, memory, load.

use super::{fit, fmt_bytes, num, pct_styled, plugin_obj, section_head, text, RenderOpts, UiState};
use super::super::term::{bar, spark};
use crate::core::value::Value;

pub(crate) fn render_header(snap: &Value, opts: &RenderOpts) -> Vec<String> {
    let host = text(snap, "system", "hostname");
    let os = text(snap, "system", "os_name");
    let upt = num(snap, "uptime", "seconds") as u64;
    let (h, m, s) = (upt / 3600, upt / 60 % 60, upt % 60);
    vec![fit(
        &format!("glances-rs {}  {}  {}  up {:02}:{:02}:{:02}", env!("CARGO_PKG_VERSION"), host, os, h, m, s),
        opts.cols,
    )]
}

pub(crate) fn render_quicklook(snap: &Value, opts: &RenderOpts) -> Vec<String> {
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

pub(crate) fn render_cpu(snap: &Value, opts: &RenderOpts, ui: &UiState) -> Vec<String> {
    let mut out = section_head(opts, "cpu");
    if let Some(obj) = plugin_obj(snap, "cpu") {
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

pub(crate) fn render_mem(snap: &Value, opts: &RenderOpts) -> Vec<String> {
    let mut out = section_head(opts, "mem");
    if plugin_obj(snap, "mem").is_some() {
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
    if plugin_obj(snap, "memswap").is_some() {
        let pct = num(snap, "memswap", "percent");
        out.push(fit(&format!("SWP {}", pct_styled(&opts.style, pct)), opts.cols));
    }
    out
}

pub(crate) fn render_load(snap: &Value, opts: &RenderOpts) -> Vec<String> {
    let mut out = section_head(opts, "load");
    if plugin_obj(snap, "load").is_some() {
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


