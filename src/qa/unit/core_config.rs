use crate::core::config::Config;

#[test]
fn empty_is_ok() {
    let c = Config::parse("").unwrap();
    assert!(c.sections.is_empty());
}

#[test]
fn parse_with_sections() {
    let txt = "[global]\nrefresh = 2\n[cpu]\nuser_careful = 50\n";
    let c = Config::parse(txt).unwrap();
    assert_eq!(c.get("global", "refresh"), Some("2"));
    assert_eq!(c.get("cpu", "user_careful"), Some("50"));
    assert_eq!(c.get_float("cpu", "user_careful"), Some(50.0));
}

#[test]
fn comments_and_blanks_ignored() {
    let c = Config::parse("# c\n\n; also c\n[x]\nk=1\n").unwrap();
    assert_eq!(c.get("x", "k"), Some("1"));
}

#[test]
fn missing_equals_errors() {
    assert!(Config::parse("[bad]\nno_value_here\n").is_err());
}

#[test]
fn missing_section_is_none() {
    let c = Config::parse("[a]\nk=1\n").unwrap();
    assert!(c.get("b", "k").is_none());
}

#[test]
fn default_section_used_before_first_header() {
    let c = Config::parse("k = v\n[a]\nx = y\n").unwrap();
    assert_eq!(c.get("default", "k"), Some("v"));
    assert_eq!(c.get("a", "x"), Some("y"));
}
