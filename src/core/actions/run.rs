//! Alert actions runner — `GlancesActions.run` + `secure_popen` parity.
//!
//! Safety posture mirrors upstream (GHSA-3vwc-qwhc-3mj7,
//! GHSA-56xw-p9qm-r437, GHSA-qcpp-8x79-hhp3):
//! - command lines are tokenized FIRST (quote-aware, no shell ever);
//! - `{{mustache}}` values render per-argument AFTER boundaries are
//!   fixed, with shell operators stripped from every value;
//! - `--disable-config-exec` reduces everything to a single process
//!   (no `&&` chaining, pipes, or `>` redirection);
//! - a startup grace timer suppresses triggers on the first ticks;
//! - non-repeat actions fire once per trigger level.
//!
//! Mustache subset: `{{key}}` / `{{{key}}}` plain substitution only —
//! section tags (`{{#`, `{{/`, `{{^`) refuse the command rather than
//! render it unsafely.

use std::collections::{BTreeMap, HashMap};
use std::process::{Command, Stdio};
use std::time::Instant;

/// Shell operators secure_popen interprets (multi-char first so `&&`
/// collapses to one space, matching upstream's strip order).
const SHELL_OPERATORS: &[&str] = &["&&", ">>", "|", ">", "&"];

/// Repeat gate + trigger memory + startup grace.
pub struct GlancesActions {
    status: HashMap<String, String>,
    start: Instant,
    grace_secs: f64,
    /// False under `--disable-config-exec`: single process, operators
    /// passed verbatim (upstream `allow_operators` parity).
    pub allow_operators: bool,
}

impl GlancesActions {
    pub fn new(refresh_secs: f32, allow_operators: bool) -> Self {
        Self {
            status: HashMap::new(),
            start: Instant::now(),
            // Upstream parity: `Timer(args.time * 2)` — no floor, so a
            // zero refresh means no grace (and is test-friendly).
            grace_secs: (refresh_secs as f64 * 2.0).max(0.0),
            allow_operators,
        }
    }

    pub fn get(&self, stat_name: &str) -> Option<&str> {
        self.status.get(stat_name).map(String::as_str)
    }

    /// Run `commands` for a trigger. Returns true when executed.
    pub fn run(
        &mut self,
        stat_name: &str,
        criticality: &str,
        commands: &[String],
        repeat: bool,
        mustache: &BTreeMap<String, String>,
    ) -> bool {
        if (self.get(stat_name) == Some(criticality) && !repeat)
            || self.start.elapsed().as_secs_f64() < self.grace_secs
        {
            return false;
        }
        for cmd in commands {
            match execute_command(cmd, mustache, self.allow_operators) {
                Ok(out) => crate::core::logger::debug(&format!(
                    "action {} for {} ({}): {}", if repeat { "repeat" } else { "run" }, stat_name, criticality, out
                )),
                Err(e) => crate::core::logger::error(&format!(
                    "action error for {} ({}): {}", stat_name, criticality, e
                )),
            }
        }
        self.status.insert(stat_name.to_string(), criticality.to_string());
        true
    }
}

/// Strip shell operators from an interpolated value (upstream
/// `_sanitize_value` parity).
pub fn sanitize_value(s: &str) -> String {
    let mut out = s.to_string();
    for op in SHELL_OPERATORS {
        out = out.replace(op, " ");
    }
    out
}

/// Render `{{key}}` / `{{{key}}}` from a sanitized dict. Section tags
/// are refused (Err) instead of rendered unsafely.
fn render_arg(arg: &str, dict: &BTreeMap<String, String>) -> Result<String, String> {
    if arg.contains("{{#") || arg.contains("{{/") || arg.contains("{{^") {
        return Err(format!("mustache sections unsupported in {:?}", arg));
    }
    let mut out = arg.to_string();
    // Triple-stash first so `{{{k}}}` isn't half-eaten by the `{{k}}` pass.
    loop {
        let Some(s) = out.find("{{{") else { break };
        let Some(e) = out[s..].find("}}}") else {
            return Err(format!("unclosed mustache tag in {:?}", arg));
        };
        let key = out[s + 3..s + e].trim();
        let val = dict.get(key).map(|v| sanitize_value(v)).unwrap_or_default();
        out.replace_range(s..s + e + 3, &val);
    }
    loop {
        let Some(s) = out.find("{{") else { break };
        let Some(e) = out[s..].find("}}") else {
            return Err(format!("unclosed mustache tag in {:?}", arg));
        };
        let key = out[s + 2..s + e].trim();
        let val = dict.get(key).map(|v| sanitize_value(v)).unwrap_or_default();
        out.replace_range(s..s + e + 2, &val);
    }
    Ok(out)
}

/// Quote-aware argv split (spaces separate except inside single or
/// double quotes; surrounding quotes stripped). Rendering happens
/// per-argument AFTER the split, never before.
fn split_args(cmd: &str, dict: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
    let mut args = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut in_arg = false;
    for c in cmd.chars() {
        match (quote, c) {
            (None, '\'') | (None, '"') => { quote = Some(c); in_arg = true; }
            (Some(q), c) if c == q => { quote = None; }
            (None, c) if c.is_whitespace() => {
                if in_arg { args.push(render_arg(&cur, dict)?); cur.clear(); in_arg = false; }
            }
            _ => { cur.push(c); in_arg = true; }
        }
    }
    if quote.is_some() {
        return Err(format!("unclosed quote in {:?}", cmd));
    }
    if in_arg {
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
        let argv = split_args(cmd, dict)?;
        return run_argv(&argv);
    }
    let mut ret = String::new();
    for segment in cmd.split("&&") {
        ret.push_str(&run_pipeline(segment, dict)?);
    }
    Ok(ret)
}

/// One `&&` segment: optional `> file` redirect + `|` pipeline.
fn run_pipeline(segment: &str, dict: &BTreeMap<String, String>) -> Result<String, String> {
    let parts: Vec<&str> = segment.split('>').collect();
    if parts.len() > 2 {
        return Err(format!("only one file redirection allowed ({})", segment.trim()));
    }
    let (cmd_part, redirect) = match parts.as_slice() {
        [c, r] => (*c, Some(r.trim().to_string())),
        [c] => (*c, None),
        _ => return Err("empty command".into()),
    };
    let redirect = match redirect {
        Some(r) if !r.is_empty() => Some(render_arg(&r, dict)?),
        Some(_) => return Err("empty redirection target".into()),
        None => None,
    };
    // Tokenize every stage BEFORE spawning anything: a template error
    // must not leave a half-started pipeline behind.
    let mut stages = Vec::new();
    for stage in cmd_part.split('|') {
        let argv = split_args(stage, dict)?;
        if argv.is_empty() || argv.iter().all(|a| a.is_empty()) {
            return Err(format!("empty pipeline stage in {:?}", segment.trim()));
        }
        stages.push(argv);
    }
    if stages.is_empty() {
        return Err("empty command".into());
    }
    let mut input: Option<std::process::ChildStdout> = None;
    let mut children = Vec::new();
    for (i, argv) in stages.iter().enumerate() {
        let last = i + 1 == stages.len();
        let mut c = Command::new(&argv[0]);
        c.args(&argv[1..]);
        c.stdin(input.take().map(Stdio::from).unwrap_or_else(Stdio::inherit));
        c.stdout(Stdio::piped());
        c.stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| format!("spawn {:?}: {}", argv, e))?;
        if !last {
            input = child.stdout.take();
        }
        children.push(child);
    }
    let last = children.pop().expect("stage");
    let out = last.wait_with_output().map_err(|e| format!("wait: {}", e))?;
    for mut c in children {
        let _ = c.wait();
    }
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    let ret = if stderr.is_empty() { stdout } else { stderr };
    if let Some(path) = redirect {
        std::fs::write(&path, &ret).map_err(|e| format!("redirect to {:?}: {}", path, e))?;
    }
    Ok(ret)
}

/// Single process, no operators (upstream `__run_argv` parity).
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
        .map_err(|e| format!("spawn {:?}: {}", argv, e))?;
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    Ok(if stderr.is_empty() { stdout } else { stderr })
}
