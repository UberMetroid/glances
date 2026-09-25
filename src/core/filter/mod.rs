//! Process filter — regex-based include/exclude matching against process
//! names and cmdlines. Mirrors `glances/filter.py` (194 LOC).
//!
//! Supports a minimal regex syntax — literals, `^`, `$`, `.`, `*`, `+`, `?`,
//! classes `[abc]`/`[^abc]`, groups `( ... )` (incl. quantified groups like
//! `(ab)*`), alternation `a|b`. No backrefs/lookaround. Implemented as a
//! small position-set matcher so we don't pull in the `regex` crate (AC-1).

use std::collections::HashSet;

use super::error::{GlancesError, Result};

mod glances;
mod parse;
use parse::parse_alt;

pub use glances::{GlancesFilter, GlancesFilterList};

pub struct ProcessFilter {
    regex: Option<Regex>,
    raw: String,
}

impl ProcessFilter {
    pub fn empty() -> Self {
        Self { regex: None, raw: String::new() }
    }

    pub fn new(raw: &str) -> Result<Self> {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return Ok(Self::empty());
        }
        let regex = Regex::compile(trimmed)?;
        Ok(Self { regex: Some(regex), raw: trimmed.to_string() })
    }

    pub fn matches(&self, name: &str, cmdline: &str) -> bool {
        match &self.regex {
            None => true,
            Some(r) => r.is_match(name) || r.is_match(cmdline),
        }
    }

    pub fn raw(&self) -> &str { &self.raw }
    pub fn is_active(&self) -> bool { self.regex.is_some() }
}

#[derive(Debug, Clone)]
enum Instr {
    Lit(char),
    Any,
    AnchorStart,
    AnchorEnd,
    /// Quantifiers hold the atom's *program* — one instr for literals/
    /// classes, several for groups — so `(ab)*` and `(a|b)+` work.
    Star(Vec<Instr>),
    Plus(Vec<Instr>),
    Quest(Vec<Instr>),
    Class { negated: bool, ranges: Vec<(char, char)> },
    Alt(Vec<Instr>, Vec<Instr>),
}

#[derive(Debug, Clone)]
pub struct Regex {
    program: Vec<Instr>,
    /// True if the pattern explicitly anchors to end-of-string (`$`).
    /// When false, partial matches anywhere in the input count.
    anchored_end: bool,
    /// True if the pattern explicitly anchors to start (`^`).
    anchored_start: bool,
}

impl Regex {
    pub fn compile(pat: &str) -> Result<Self> {
        let chars: Vec<char> = pat.chars().collect();
        let prog = parse_alt(&chars, 0).0
            .ok_or_else(|| GlancesError::Parse("regex: empty pattern".into()))?;
        let anchored_start = matches!(prog.first(), Some(Instr::AnchorStart));
        let anchored_end = matches!(prog.last(), Some(Instr::AnchorEnd));
        Ok(Self { program: prog, anchored_start, anchored_end })
    }

    pub fn is_match(&self, s: &str) -> bool {
        let chars: Vec<char> = s.chars().collect();
        let n = chars.len();
        let start_positions: Box<dyn Iterator<Item = usize>> = if self.anchored_start {
            Box::new(0..1)
        } else {
            Box::new(0..=n)
        };
        for start in start_positions {
            let mut out = Vec::new();
            let mut seen = HashSet::new();
            collect_positions(&self.program, &chars, start, n, &mut out, &mut seen);
            let hit = if self.anchored_end { out.contains(&n) } else { !out.is_empty() };
            if hit { return true; }
        }
        false
    }

    /// Full-string match (upstream `re.fullmatch` parity for process
    /// filters): the pattern must consume the entire input, regardless
    /// of `^`/`$` anchors in the pattern itself.
    pub fn is_full_match(&self, s: &str) -> bool {
        let chars: Vec<char> = s.chars().collect();
        let n = chars.len();
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        collect_positions(&self.program, &chars, 0, n, &mut out, &mut seen);
        out.contains(&n)
    }
}

fn class_hit(negated: bool, ranges: &[(char, char)], c: char) -> bool {
    let hit = ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi);
    if negated { !hit } else { hit }
}

/// Append every position reachable by fully matching `prog` starting at
/// `pos` into `out`. `seen` memoizes (program-offset, input-pos) pairs
/// already expanded so `*` over empty-matching atoms can't loop and
/// nested repeats stay polynomial.
fn collect_positions(
    prog: &[Instr],
    s: &[char],
    pos: usize,
    end: usize,
    out: &mut Vec<usize>,
    seen: &mut HashSet<(usize, usize, usize)>,
) {
    if prog.is_empty() {
        out.push(pos);
        return;
    }
    // Key includes length: same-address/different-length program tails
    // must not alias each other in the memo.
    if !seen.insert((prog.as_ptr() as usize, prog.len(), pos)) {
        return;
    }
    match &prog[0] {
        Instr::Lit(c) if pos < end && s[pos] == *c => {
            collect_positions(&prog[1..], s, pos + 1, end, out, seen);
        }
        Instr::Any if pos < end && s[pos] != '\n' => {
            collect_positions(&prog[1..], s, pos + 1, end, out, seen);
        }
        Instr::AnchorStart if pos == 0 => {
            collect_positions(&prog[1..], s, pos, end, out, seen);
        }
        Instr::AnchorEnd if pos == end => {
            collect_positions(&prog[1..], s, pos, end, out, seen);
        }
        Instr::Class { negated, ranges }
            if pos < end && class_hit(*negated, ranges, s[pos]) =>
        {
            collect_positions(&prog[1..], s, pos + 1, end, out, seen);
        }
        Instr::Alt(a, b) => {
            // Each branch must continue into the pattern *after* the
            // alternation — `x(a|b)y` requires the `y` too.
            for branch in [a, b] {
                let mut p = branch.clone();
                p.extend_from_slice(&prog[1..]);
                collect_positions(&p, s, pos, end, out, seen);
            }
        }
        Instr::Star(atom) => {
            collect_positions(&prog[1..], s, pos, end, out, seen); // 0 reps
            for p in one_rep(atom, s, pos, end, seen) {
                // p > pos: another rep consumed input — re-enter the
                // whole program (still headed by this Star). p == pos
                // means a zero-width rep, already covered by 0 reps.
                if p > pos { collect_positions(prog, s, p, end, out, seen); }
            }
        }
        Instr::Plus(atom) => {
            // 1+ reps = one rep then `atom*` before the rest.
            let mut rest = vec![Instr::Star(atom.clone())];
            rest.extend_from_slice(&prog[1..]);
            for p in one_rep(atom, s, pos, end, seen) {
                collect_positions(&rest, s, p, end, out, seen);
            }
        }
        Instr::Quest(atom) => {
            collect_positions(&prog[1..], s, pos, end, out, seen); // 0 reps
            for p in one_rep(atom, s, pos, end, seen) {
                collect_positions(&prog[1..], s, p, end, out, seen);
            }
        }
        _ => {} // guarded arm above failed (e.g. pos at end of input)
    }
}

/// Positions reachable by matching the atom program exactly once.
fn one_rep(
    atom: &[Instr],
    s: &[char],
    pos: usize,
    end: usize,
    seen: &mut HashSet<(usize, usize, usize)>,
) -> Vec<usize> {
    let mut v = Vec::new();
    collect_positions(atom, s, pos, end, &mut v, seen);
    v.sort_unstable();
    v.dedup();
    v
}
