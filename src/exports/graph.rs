//! Graph exporter — renders per-plugin SVG time-series charts.
//!
//! Mirrors `glances/exports/glances_graph/__init__.py` (pygal
//! `DateTimeLine`), minus the dependency: SVG is plain XML, so the port
//! draws one `<polyline>` per series itself. Each `write` appends the
//! current numeric fields to an in-process series store (capped at
//! `max_points`, subsampled to `width` like upstream's
//! `time_series_subsample`) and rewrites `<path>/<plugin>.svg`.
//!
//! Upstream generates charts on demand (`generate_every` / `g` key);
//! the port regenerates every tick the exporter runs.

use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};

use crate::core::error::Result;
use crate::core::value::Value;
use crate::exports::flatten::Field;

pub const NAME: &str = "graph";

/// Points kept per series before old samples are dropped.
pub const DEFAULT_MAX_POINTS: usize = 300;

#[derive(Debug, Clone)]
pub struct Config {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub max_points: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            path: "graphs".into(),
            width: 800,
            height: 600,
            max_points: DEFAULT_MAX_POINTS,
        }
    }
}

fn now_secs() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs_f64())
        .unwrap_or(0.0)
}

/// In-process series store: `"series.key"` → (timestamp, value) samples.
fn store() -> &'static Mutex<BTreeMap<String, Vec<(f64, f64)>>> {
    static S: OnceLock<Mutex<BTreeMap<String, Vec<(f64, f64)>>>> = OnceLock::new();
    S.get_or_init(|| Mutex::new(BTreeMap::new()))
}

/// Append current numeric fields to the series store (shared helper so
/// tests can drive rendering without touching globals).
pub fn append_fields(
    map: &mut BTreeMap<String, Vec<(f64, f64)>>,
    fields: &[Field<'_>],
    ts: f64,
    max_points: usize,
) {
    for f in fields {
        let n = match f.value {
            Value::Int(i) => Some(*i as f64),
            Value::Uint(u) => Some(*u as f64),
            Value::Float(v) if v.is_nan() || v.is_infinite() => None,
            Value::Float(v) => Some(*v),
            _ => None,
        };
        if let Some(n) = n {
            let key = format!("{}.{}", f.series, f.key);
            let series = map.entry(key).or_default();
            series.push((ts, n));
            if series.len() > max_points {
                let drop = series.len() - max_points;
                series.drain(..drop);
            }
        }
    }
}

const PALETTE: [&str; 8] = [
    "#e41a1c", "#377eb8", "#4daf4a", "#984ea3", "#ff7f00", "#a65628", "#f781bf", "#0072b2",
];

/// Minimal XML escaping for titles/legend text.
pub fn escape_xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            _ => out.push(c),
        }
    }
    out
}

/// Subsample a series down to at most `max` points (even stride, always
/// keeping the latest sample).
pub fn subsample(points: &[(f64, f64)], max: usize) -> Vec<(f64, f64)> {
    if points.len() <= max || max == 0 {
        return points.to_vec();
    }
    let step = points.len() / max;
    let mut out: Vec<(f64, f64)> = points.iter().step_by(step.max(1)).cloned().collect();
    if *out.last().unwrap() != *points.last().unwrap() {
        out.push(*points.last().unwrap());
    }
    out
}

/// Render one plugin chart: polylines for `series` (name → samples).
pub fn render_svg(
    title: &str,
    series: &[(String, Vec<(f64, f64)>)],
    width: u32,
    height: u32,
) -> String {
    let (w, h) = (width.max(100) as f64, height.max(100) as f64);
    let pad = 44.0;
    let mut tmin = f64::INFINITY;
    let mut tmax = f64::NEG_INFINITY;
    let mut vmin = f64::INFINITY;
    let mut vmax = f64::NEG_INFINITY;
    for (_, pts) in series {
        for (t, v) in pts {
            if *t < tmin {
                tmin = *t;
            }
            if *t > tmax {
                tmax = *t;
            }
            if *v < vmin {
                vmin = *v;
            }
            if *v > vmax {
                vmax = *v;
            }
        }
    }
    if !tmin.is_finite() {
        tmin = 0.0;
        tmax = 1.0;
        vmin = 0.0;
        vmax = 1.0;
    }
    if (tmax - tmin) <= 0.0 {
        tmax = tmin + 1.0;
    }
    if (vmax - vmin) <= 0.0 {
        vmax = vmin + 1.0;
    }
    let x = |t: f64| pad + (t - tmin) / (tmax - tmin) * (w - 2.0 * pad);
    let y = |v: f64| h - pad - (v - vmin) / (vmax - vmin) * (h - 2.0 * pad);
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">\n<title>{}</title>\n<rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#1e1e1e\"/>\n",
        width,
        height,
        width,
        height,
        escape_xml(title),
        width,
        height
    );
    for (i, (name, pts)) in series.iter().enumerate() {
        let sub = subsample(pts, width as usize);
        let points: Vec<String> =
            sub.iter().map(|(t, v)| format!("{:.1},{:.1}", x(*t), y(*v))).collect();
        let color = PALETTE[i % PALETTE.len()];
        svg.push_str(&format!(
            "<polyline points=\"{}\" fill=\"none\" stroke=\"{}\" stroke-width=\"1.5\"/>\n",
            points.join(" "),
            color
        ));
        svg.push_str(&format!(
            "<text x=\"{}\" y=\"{}\" fill=\"{}\" font-size=\"12\">{}</text>\n",
            pad + 4.0,
            16.0 + i as f64 * 15.0,
            color,
            escape_xml(name)
        ));
    }
    svg.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" fill=\"#aaaaaa\" font-size=\"11\">max {:.2}</text>\n",
        4.0,
        pad - 6.0,
        vmax
    ));
    svg.push_str(&format!(
        "<text x=\"{}\" y=\"{}\" fill=\"#aaaaaa\" font-size=\"11\">min {:.2}</text>\n",
        4.0,
        h - 4.0,
        vmin
    ));
    svg.push_str("</svg>\n");
    svg
}

/// Write one `<plugin>.svg` per plugin present in `fields`, returning the
/// files written. Pure rendering core over an explicit series map (the
/// global store is only touched by `write`).
pub fn render_all(
    map: &BTreeMap<String, Vec<(f64, f64)>>,
    fields: &[Field<'_>],
    cfg: &Config,
) -> Vec<(String, String)> {
    let mut by_plugin: BTreeMap<&str, Vec<(String, Vec<(f64, f64)>)>> = BTreeMap::new();
    for f in fields {
        let key = format!("{}.{}", f.series, f.key);
        if let Some(pts) = map.get(&key) {
            if pts.is_empty() {
                continue;
            }
            by_plugin
                .entry(f.plugin)
                .or_default()
                .push((f.key.to_string(), pts.clone()));
        }
    }
    by_plugin
        .into_iter()
        .map(|(plugin, series)| {
            let name = format!("{}.svg", plugin);
            (name, render_svg(plugin, &series, cfg.width, cfg.height))
        })
        .collect()
}

pub fn write(fields: &[Field<'_>], cfg: &Config) -> Result<()> {
    let ts = now_secs();
    let rendered = {
        let mut guard = store().lock().unwrap_or_else(|e| e.into_inner());
        append_fields(&mut guard, fields, ts, cfg.max_points);
        render_all(&guard, fields, cfg)
    };
    std::fs::create_dir_all(&cfg.path)?;
    for (name, svg) in rendered {
        std::fs::write(std::path::Path::new(&cfg.path).join(name), svg.as_bytes())?;
    }
    Ok(())
}
