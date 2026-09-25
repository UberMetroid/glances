//! Alert command runner: fire configured commands on triggers.
//!
//! Safety rules (non-negotiable):
//! - commands tokenize FIRST with a quote-aware splitter; no shell ever;
//! - `{{key}}` values render per argument AFTER boundaries are fixed,
//!   with shell operators stripped from every value;
//! - section tags refuse the command instead of rendering unsafely;
//! - single-process mode disables `&&` chains, pipes, and redirects;
//! - a startup grace suppresses early triggers;
//! - non-repeat commands fire once per trigger level.

use std::collections::{BTreeMap, HashMap};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Operators stripped from interpolated values (longest first so `&&`
/// and `>>` collapse before their prefixes).
const SHELL_OPERATORS: &[&str] = &["&&", ">>", "|", ">", "&"];

/// Trigger memory plus startup grace plus execution mode.
pub struct GlancesActions {
    status: HashMap<String, String>,
    start: Instant,
    grace_secs: f64,
    /// False under `--disable-config-exec`: one process, no operators.
    pub allow_operators: bool,
}

impl GlancesActions {
    pub fn new(refresh_secs: f32, allow_operators: bool) -> Self {
        Self {
            status: HashMap::new(),
            start: Instant::now(),
            grace_secs: (refresh_secs as f64 * 2.0).max(0.0),
            allow_operators,
        }
    }

    pub fn get(&self, stat_name: &str) -> Option<&str> {
        self.status.get(stat_name).map(String::as_str)
    }

    /// Run every command for a trigger. True when executed (failures are
    /// logged, never raised); false when gated or still in grace.
    pub fn run(
        &mut self,
        stat_name: &str,
        criticality: &str,
        commands: &[String],
        repeat: bool,
        mustache: &BTreeMap<String, String>,
    ) -> bool {
        let gated = self.get(stat_name) == Some(criticality) && !repeat;
        let in_grace = self.start.elapsed().as_secs_f64() < self.grace_secs;
        if gated || in_grace {
            return false;
        }
        let kind = if repeat { "repeat" } else { "run" };
        for cmd in commands {
            match execute_command(cmd, mustache, self.allow_operators) {
                Ok(out) => crate::core::logger::debug(&format!(
                    "action {kind} for {stat_name} ({criticality}): {out}"
                )),
                Err(e) => crate::core::logger::error(&format!(
                    "action error for {stat_name} ({criticality}): {e}"
                )),
            }
        }
        self.status.insert(stat_name.to_string(), criticality.to_string());
        true
    }
}

/// Strip shell operators from one interpolated value.
pub fn sanitize_value(s: &str) -> String {
    SHELL_OPERATORS.iter().fold(s.to_string(), |acc, op| acc.replace(op, " "))
}

/// Render `{{key}}` and `{{{key}}}` tags from the dict (missing keys
/// render empty; values sanitize). Section tags and unclosed tags are
/// errors, never rendered.
fn render_arg(arg: &str, dict: &BTreeMap<String, String>) -> Result<String, String> {
    if ["{{#", "{{/", "{{^"].iter().any(|t| arg.contains(t)) {
        return Err(format!("mustache sections unsupported in {arg:?}"));
    }
    let mut out = String::with_capacity(arg.len());
    let mut rest = arg;
    while let Some(open) = rest.find("{{") {
        let triple = rest[open..].starts_with("{{{");
        let (skip, close) = if triple { (3, "}}}") } else { (2, "}}") };
        let after = open + skip;
        let Some(rel) = rest[after..].find(close) else {
            return Err(format!("unclosed mustache tag in {arg:?}"));
        };
        out.push_str(&rest[..open]);
        let key = rest[after..after + rel].trim();
        out.push_str(&dict.get(key).map(|v| sanitize_value(v)).unwrap_or_default());
        rest = &rest[after + rel + close.len()..];
    }
    out.push_str(rest);
    Ok(out)
}

/// Quote-aware argv split; each argument renders AFTER the split.
/// Unclosed quotes are errors.
fn split_args(cmd: &str, dict: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut open = false;
    for c in cmd.chars() {
        if let Some(q) = quote {
            if c == q {
                quote = None;
            } else {
                cur.push(c);
            }
            continue;
        }
        if c == '\'' || c == '"' {
            quote = Some(c);
            open = true;
        } else if c.is_whitespace() {
            if open {
                args.push(render_arg(&cur, dict)?);
                cur.clear();
                open = false;
            }
        } else {
            cur.push(c);
            open = true;
        }
    }
    if quote.is_some() {
        return Err(format!("unclosed quote in {cmd:?}"));
    }
    if open {
        args.push(render_arg(&cur, dict)?);
    }
    Ok(args)
}

fn execute_command(
    cmd: &str,
    dict: &BTreeMap<String, String>,
    allow_operators: bool,
) -> Result<String, String> {
    if !allow_operators {
        return run_argv(&split_args(cmd, dict)?);
    }
    let mut combined = String::new();
    for segment in cmd.split("&&") {
        combined.push_str(&run_pipeline(segment, dict)?);
    }
    Ok(combined)
}

/// One `&&` segment: optional `> file` redirect plus a `|` pipeline.
/// Every stage tokenizes BEFORE anything spawns, so a template error
/// never leaves a half-started pipeline behind.
fn run_pipeline(segment: &str, dict: &BTreeMap<String, String>) -> Result<String, String> {
    let mut halves = segment.splitn(2, '>');
    let cmd_part = halves.next().unwrap_or("");
    let redirect = match halves.next().map(str::trim) {
        None => None,
        Some("") => return Err("empty redirection target".into()),
        Some(r) if r.contains('>') => {
            return Err(format!("only one file redirection allowed ({})", segment.trim()));
        }
        Some(r) => Some(render_arg(r, dict)?),
    };
    let mut stages = Vec::new();
    for stage in cmd_part.split('|') {
        let argv = split_args(stage, dict)?;
        if argv.is_empty() || argv.iter().all(String::is_empty) {
            return Err(format!("empty pipeline stage in {:?}", segment.trim()));
        }
        stages.push(argv);
    }
    if stages.is_empty() {
        return Err("empty command".into());
    }
    let mut input: Option<std::process::ChildStdout> = None;
    let mut kids = Vec::new();
    for (i, argv) in stages.iter().enumerate() {
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..]);
        c.stdin(input.take().map(Stdio::from).unwrap_or_else(Stdio::inherit));
        c.stdout(Stdio::piped());
        c.stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| format!("spawn {argv:?}: {e}"))?;
        if i + 1 < stages.len() {
            input = child.stdout.take();
        }
        kids.push(child);
    }
    let last = kids.pop().expect("stage");
    let out = last.wait_with_output().map_err(|e| format!("wait: {e}"))?;
    for mut kid in kids {
        let _ = kid.wait();
    }
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let ret = if stderr.is_empty() { stdout } else { stderr };
    if let Some(path) = redirect {
        std::fs::write(&path, &ret).map_err(|e| format!("redirect to {path:?}: {e}"))?;
    }
    Ok(ret)
}

/// One process, operators passed through as literal arguments.
fn run_argv(argv: &[String]) -> Result<String, String> {
    if argv.is_empty() {
        return Err("empty command".into());
    }
    let out = Command::new(&argv[0])
        .args(&argv[1..])
        .stdin(Stdio::inherit())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("spawn {argv:?}: {e}"))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    Ok(if stderr.is_empty() { stdout } else { stderr })
}
