use crate::core::filter::ProcessFilter;

#[test]
fn empty_matches_all() {
    let f = ProcessFilter::empty();
    assert!(f.matches("anything", "cmd"));
    assert!(!f.is_active());
}

#[test]
fn anchored_match() {
    let f = ProcessFilter::new("^nginx").unwrap();
    assert!(f.matches("nginx", ""));
    // ^nginx anchors start but not end — matches "nginx-worker" too (Python re.search).
    assert!(f.matches("nginx-worker", ""));
    // But ^nginx$ would not.
    let f2 = ProcessFilter::new("^nginx$").unwrap();
    assert!(f2.matches("nginx", ""));
    assert!(!f2.matches("nginx-worker", ""));
}

#[test]
fn unanchored_match() {
    let f = ProcessFilter::new("nginx").unwrap();
    assert!(f.matches("nginx", ""));
    assert!(f.matches("nginx-worker", ""));
}

#[test]
fn alternation() {
    let f = ProcessFilter::new("nginx|apache").unwrap();
    eprintln!("debug: alt nginx={}", f.matches("nginx", ""));
    eprintln!("debug: alt apache2={}", f.matches("apache2", ""));
    assert!(f.matches("nginx", ""));
    assert!(f.matches("apache2", ""));
    assert!(!f.matches("mysql", ""));
}

#[test]
fn any_wildcard() {
    let f = ProcessFilter::new("ngin.").unwrap();
    assert!(f.matches("nginx", ""));
    assert!(!f.matches("ngin", ""));
}

#[test]
fn character_class() {
    let f = ProcessFilter::new("[nm]ginx").unwrap();
    assert!(f.matches("nginx", ""));
    assert!(f.matches("mginx", ""));
    assert!(!f.matches("xginx", ""));
}

#[test]
fn negation_class() {
    let f = ProcessFilter::new("[^a]ginx").unwrap();
    assert!(f.matches("bginx", ""));
    assert!(!f.matches("agninx", ""));
}

#[test]
fn matches_cmdline() {
    let f = ProcessFilter::new("python").unwrap();
    eprintln!("debug: cmdline python3={}", f.matches("python3", "/usr/bin/python3 -m http.server"));
    assert!(f.matches("python3", "/usr/bin/python3 -m http.server"));
}

#[test]
fn star_quantifier() {
    let f = ProcessFilter::new("go.*").unwrap();
    assert!(f.matches("go", ""));
    assert!(f.matches("go-http", ""));
}

#[test]
fn plus_quantifier() {
    let f = ProcessFilter::new("go+").unwrap();
    assert!(f.matches("go", ""));
    assert!(f.matches("goo", ""));
    assert!(!f.matches("g", ""));
}

#[test]
fn alternation_continues_pattern() {
    // Regression: `x(a|b)y` must match "xay"/"xby" only — not "xab".
    let f = ProcessFilter::new("x(a|b)y").unwrap();
    assert!(f.matches("xay", ""));
    assert!(f.matches("xby", ""));
    assert!(!f.matches("xab", ""));
    assert!(!f.matches("xa", ""));
}

#[test]
fn anchored_alternation_continues_pattern() {
    // Regression: `x(a|b)y$` could never match because branch programs
    // were checked with the outer must_reach_end mid-string.
    let f = ProcessFilter::new("x(a|b)y$").unwrap();
    assert!(f.matches("xay", ""));
    assert!(f.matches("xby", ""));
    assert!(!f.matches("xayz", ""));
    assert!(!f.matches("xab", ""));
}

#[test]
fn group_quantifiers() {
    // Regression: `(ab)*` must repeat the group — the old code spliced
    // the group contents so the `*` bound to `b` only. Anchor it to
    // check the repetition actually applies to the whole group.
    // (matches() ORs name and cmdline — "zz" cmdline can't match `^...$`)
    let f = ProcessFilter::new("^(ab)*$").unwrap();
    assert!(f.matches("abab", "zz"));
    assert!(f.matches("ab", "zz"));
    assert!(f.matches("", "zz"));
    assert!(!f.matches("ab*", "zz"));
    assert!(!f.matches("abb", "zz"));
    let g = ProcessFilter::new("x(ab)+y").unwrap();
    assert!(g.matches("xaby", ""));
    assert!(g.matches("xababy", ""));
    assert!(!g.matches("xy", ""));
    let q = ProcessFilter::new("a(bc)?d").unwrap();
    assert!(q.matches("ad", ""));
    assert!(q.matches("abcd", ""));
    assert!(!q.matches("abcbcd", ""));
}
