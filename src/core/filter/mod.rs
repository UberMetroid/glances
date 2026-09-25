//! Process matching: include/exclude filters over names and cmdlines.
//!
//! Patterns use a small built-in syntax — literals, `^$`, `.`, `*+?`,
//! `[abc]`/`[^a-z]`, `(groups)`, `a|b` — matched by a backtracking
//! engine over compiled instructions (see `parse`). No backrefs or
//! lookaround, and no outside crates.

use std::collections::HashSet;

use super::error::{GlancesError, Result};

mod glances;
mod parse;
use parse::parse_alt;

pub use glances::{GlancesFilter, GlancesFilterList};

/// A compiled include/exclude rule. Inactive (empty) filters match
/// everything; active ones match when the name OR the cmdline matches.
pub struct ProcessFilter {
    regex: Option<Regex>,
    raw: String,
}

impl ProcessFilter {
    pub fn empty() -> Self {
        Self { regex: None, raw: String::new() }
    }

    pub fn new(raw: &str) -> Result<Self> {
        let text = raw.trim();
        if text.is_empty() {
            return Ok(Self::empty());
        }
        Ok(Self { regex: Some(Regex::compile(text)?), raw: text.to_string() })
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

/// Compiled pattern instructions. Quantifiers carry the sub-program of
/// their atom so quantified groups and alternations expand correctly.
#[derive(Debug, Clone)]
enum Instr {
    Lit(char),
    Any,
    AnchorStart,
    AnchorEnd,
    Star(Vec<Instr>),
    Plus(Vec<Instr>),
    Quest(Vec<Instr>),
    Class { negated: bool, ranges: Vec<(char, char)> },
    Alt(Vec<Instr>, Vec<Instr>),
}

#[derive(Debug, Clone)]
pub struct Regex {
    program: Vec<Instr>,
    anchored_end: bool,
    anchored_start: bool,
}

impl Regex {
    pub fn compile(pat: &str) -> Result<Self> {
        let chars: Vec<char> = pat.chars().collect();
        let program = parse_alt(&chars, 0).0
            .ok_or_else(|| GlancesError::Parse("regex: empty pattern".into()))?;
        let anchored_start = matches!(program.first(), Some(Instr::AnchorStart));
        let anchored_end = matches!(program.last(), Some(Instr::AnchorEnd));
        Ok(Self { program, anchored_start, anchored_end })
    }

    /// True when the pattern matches anywhere in `s` (or at position 0
    /// when `^`-anchored). `$`-anchored patterns must reach the end.
    pub fn is_match(&self, s: &str) -> bool {
        let chars: Vec<char> = s.chars().collect();
        let mut starts: Vec<usize> = (0..=chars.len()).collect();
        if self.anchored_start {
            starts.truncate(1);
        }
        let mut m = Matcher::new(&chars);
        starts.into_iter().any(|at| {
            let ends = m.run(&self.program, 0, at);
            self.anchored_end && ends.contains(&chars.len()) || !self.anchored_end && !ends.is_empty()
        })
    }

    /// True only when the whole input is consumed from position 0,
    /// regardless of anchors written in the pattern.
    pub fn is_full_match(&self, s: &str) -> bool {
        let chars: Vec<char> = s.chars().collect();
        Matcher::new(&chars).run(&self.program, 0, 0).contains(&chars.len())
    }
}

/// Backtracking matcher. `run` returns every input position reachable by
/// fully matching a program tail from `pos`. The `seen` set keys on
/// (region, tail length, position) so empty-matching loops expand once
/// and nested repeats stay polynomial; every synthesized program (branch
/// splices) mints a fresh region id.
struct Matcher<'a> {
    input: &'a [char],
    seen: HashSet<(usize, usize, usize)>,
    next_region: usize,
}

impl<'a> Matcher<'a> {
    fn new(input: &'a [char]) -> Self {
        Self { input, seen: HashSet::new(), next_region: 1 }
    }

    fn region(&mut self) -> usize {
        let r = self.next_region;
        self.next_region += 1;
        r
    }

    fn run(&mut self, prog: &[Instr], region: usize, pos: usize) -> Vec<usize> {
        if prog.is_empty() {
            return vec![pos];
        }
        if !self.seen.insert((region, prog.len(), pos)) {
            return Vec::new();
        }
        let end = self.input.len();
        let at_end = pos >= end;
        match &prog[0] {
            Instr::Lit(c) if !at_end && self.input[pos] == *c => self.run(&prog[1..], region, pos + 1),
            Instr::Any if !at_end && self.input[pos] != '\n' => self.run(&prog[1..], region, pos + 1),
            Instr::AnchorStart if pos == 0 => self.run(&prog[1..], region, pos),
            Instr::AnchorEnd if pos == end => self.run(&prog[1..], region, pos),
            Instr::Class { negated, ranges } if !at_end && class_accepts(*negated, ranges, self.input[pos]) => {
                self.run(&prog[1..], region, pos + 1)
            }
            Instr::Alt(a, b) => {
                // Each branch continues into the tail after the alternation.
                let mut hits = Vec::new();
                for branch in [a, b] {
                    let mut spliced = branch.clone();
                    spliced.extend_from_slice(&prog[1..]);
                    let region = self.region();
                    hits.extend(self.run(&spliced, region, pos));
                }
                hits
            }
            Instr::Star(atom) => {
                let mut hits = self.run(&prog[1..], region, pos);
                for p in self.once(atom, pos) {
                    // Progressing reps re-enter through this Star; a
                    // zero-width rep adds nothing beyond zero reps.
                    if p > pos {
                        hits.extend(self.run(prog, region, p));
                    }
                }
                hits
            }
            Instr::Plus(atom) => {
                // One rep, then the atom starred ahead of the tail.
                let mut rest = vec![Instr::Star(atom.clone())];
                rest.extend_from_slice(&prog[1..]);
                let rest_region = self.region();
                let mut hits = Vec::new();
                for p in self.once(atom, pos) {
                    hits.extend(self.run(&rest, rest_region, p));
                }
                hits
            }
            Instr::Quest(atom) => {
                let mut hits = self.run(&prog[1..], region, pos);
                for p in self.once(atom, pos) {
                    hits.extend(self.run(&prog[1..], region, p));
                }
                hits
            }
            _ => Vec::new(),
        }
    }

    /// Positions reachable by matching an atom program exactly once.
    /// Atoms live in their own region so their tails can never alias an
    /// enclosing tail in the memo.
    fn once(&mut self, atom: &[Instr], pos: usize) -> Vec<usize> {
        let region = self.region();
        let mut v = self.run(atom, region, pos);
        v.sort_unstable();
        v.dedup();
        v
    }
}

fn class_accepts(negated: bool, ranges: &[(char, char)], c: char) -> bool {
    let inside = ranges.iter().any(|&(lo, hi)| (lo..=hi).contains(&c));
    inside != negated
}
