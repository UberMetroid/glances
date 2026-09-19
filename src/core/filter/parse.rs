//! Pattern parser for `core::filter` (split out for the file-size
//! lint). Produces the `Instr` program consumed by the matcher.

use super::Instr;

pub(super) fn parse_alt(s: &[char], i: usize) -> (Option<Vec<Instr>>, usize) {
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
    let (base, j): (Vec<Instr>, usize) = match c {
        '^' => (vec![Instr::AnchorStart], i + 1),
        '$' => (vec![Instr::AnchorEnd], i + 1),
        '.' => (vec![Instr::Any], i + 1),
        '[' => match parse_class(s, i) { (Some(cl), k) => (vec![cl], k), (None, k) => return (None, k) },
        '\\' if i + 1 < s.len() => (vec![Instr::Lit(s[i + 1])], i + 2),
        '(' => {
            let (inner, k) = match parse_alt(s, i + 1) {
                (Some(p), k) => (p, k),
                (None, k) => return (None, k),
            };
            if k >= s.len() || s[k] != ')' { return (None, k); }
            (inner, k + 1)
        }
        other => (vec![Instr::Lit(other)], i + 1),
    };
    if j < s.len() {
        match s[j] {
            '*' => return (Some(vec![Instr::Star(base)]), j + 1),
            '+' => return (Some(vec![Instr::Plus(base)]), j + 1),
            '?' => return (Some(vec![Instr::Quest(base)]), j + 1),
            _ => {}
        }
    }
    (Some(base), j)
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
