//! Process filter — regex-based include/exclude matching against process
//! names and cmdlines. Mirrors `glances/filter.py` (194 LOC).
//!
//! Supports a minimal regex syntax — literals, `^`, `$`, `.`, `*`, `+`, `?`,
//! character classes `[abc]` / `[^abc]`, alternation `a|b`. No backrefs, no
//! lookaround. Implemented as a small recursive matcher so we don't pull in
//! the `regex` crate (AC-1).

use super::error::{GlancesError, Result};

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
    Star(Box<Instr>),
    Plus(Box<Instr>),
    Quest(Box<Instr>),
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
            if try_match(&self.program, &chars, start, n, self.anchored_end) {
                return true;
            }
        }
        false
    }
}

fn try_match(prog: &[Instr], s: &[char], pos: usize, end: usize, must_reach_end: bool) -> bool {
    if prog.is_empty() {
        return if must_reach_end { pos == end } else { true };
    }
    match &prog[0] {
        Instr::Lit(c) => pos < end && s[pos] == *c && try_match(&prog[1..], s, pos + 1, end, must_reach_end),
        Instr::Any => pos < end && s[pos] != '\n' && try_match(&prog[1..], s, pos + 1, end, must_reach_end),
        Instr::AnchorStart => pos == 0 && try_match(&prog[1..], s, pos, end, must_reach_end),
        Instr::AnchorEnd => pos == end && try_match(&prog[1..], s, pos, end, must_reach_end),
        Instr::Star(x) => {
            let mut p = pos;
            while p < end && single_matches(x, s, p) { p += 1; }
            loop {
                if try_match(&prog[1..], s, p, end, must_reach_end) { return true; }
                if p == pos { return false; }
                p -= 1;
            }
        }
        Instr::Plus(x) => {
            if pos >= end || !single_matches(x, s, pos) { return false; }
            let mut p = pos + 1;
            while p < end && single_matches(x, s, p) { p += 1; }
            loop {
                if try_match(&prog[1..], s, p, end, must_reach_end) { return true; }
                if p == pos + 1 { return false; }
                p -= 1;
            }
        }
        Instr::Quest(x) =>
            try_match(&prog[1..], s, pos, end, must_reach_end) ||
                (pos < end && single_matches(x, s, pos) && try_match(&prog[1..], s, pos + 1, end, must_reach_end)),
        Instr::Class { negated, ranges } => {
            pos < end && {
                let c = s[pos];
                let hit = ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi);
                let ok = if *negated { !hit } else { hit };
                ok && try_match(&prog[1..], s, pos + 1, end, must_reach_end)
            }
        }
        Instr::Alt(a, b) => try_match(a, s, pos, end, must_reach_end) || try_match(b, s, pos, end, must_reach_end),
    }
}

fn single_matches(instr: &Instr, s: &[char], pos: usize) -> bool {
    if pos >= s.len() { return false; }
    match instr {
        Instr::Lit(c) => s[pos] == *c,
        Instr::Any => s[pos] != '\n',
        Instr::Class { negated, ranges } => {
            let c = s[pos];
            let hit = ranges.iter().any(|&(lo, hi)| c >= lo && c <= hi);
            if *negated { !hit } else { hit }
        }
        _ => false,
    }
}

fn parse_alt(s: &[char], i: usize) -> (Option<Vec<Instr>>, usize) {
    let (mut left, mut j) = match parse_cat(s, i) {
        (Some(p), j) => (p, j),
        (None, j) => return (None, j),
    };
    while j < s.len() && s[j] == '|' {
        let (right, k) = match parse_cat(s, j + 1) {
            (Some(p), k) => (p, k),
            (None, k) => return (None, k),
        };
        left = vec![Instr::Alt(left, right)];
        j = k;
    }
    (Some(left), j)
}

fn parse_cat(s: &[char], i: usize) -> (Option<Vec<Instr>>, usize) {
    let mut out: Vec<Instr> = Vec::new();
    let mut j = i;
    while j < s.len() && s[j] != '|' && s[j] != ')' {
        let (instr, k) = match parse_atom(s, j) {
            (Some(p), k) => (p, k),
            (None, k) => return (None, k),
        };
        out.extend(instr);
        j = k;
    }
    if out.is_empty() { (None, j) } else { (Some(out), j) }
}

fn parse_atom(s: &[char], i: usize) -> (Option<Vec<Instr>>, usize) {
    if i >= s.len() { return (None, i); }
    let c = s[i];
    let (base, j) = match c {
        '^' => (Instr::AnchorStart, i + 1),
        '$' => (Instr::AnchorEnd, i + 1),
        '.' => (Instr::Any, i + 1),
        '[' => match parse_class(s, i) { (Some(cl), k) => (cl, k), (None, k) => return (None, k) },
        '\\' if i + 1 < s.len() => (Instr::Lit(s[i + 1]), i + 2),
        '(' => {
            let (inner, k) = match parse_alt(s, i + 1) {
                (Some(p), k) => (p, k),
                (None, k) => return (None, k),
            };
            if k >= s.len() || s[k] != ')' { return (None, k); }
            return (Some(inner), k + 1);
        }
        other => (Instr::Lit(other), i + 1),
    };
    if j < s.len() {
        match s[j] {
            '*' => return (Some(vec![Instr::Star(Box::new(base))]), j + 1),
            '+' => return (Some(vec![Instr::Plus(Box::new(base))]), j + 1),
            '?' => return (Some(vec![Instr::Quest(Box::new(base))]), j + 1),
            _ => {}
        }
    }
    (Some(vec![base]), j)
}

fn parse_class(s: &[char], i: usize) -> (Option<Instr>, usize) {
    let mut j = i + 1;
    let negated = j < s.len() && s[j] == '^';
    if negated { j += 1; }
    let mut ranges = Vec::new();
    while j < s.len() && s[j] != ']' {
        let lo = s[j];
        if j + 2 < s.len() && s[j + 1] == '-' && s[j + 2] != ']' {
            ranges.push((lo, s[j + 2]));
            j += 3;
        } else {
            ranges.push((lo, lo));
            j += 1;
        }
    }
    if j >= s.len() { return (None, i); }
    (Some(Instr::Class { negated, ranges }), j + 1)
}
