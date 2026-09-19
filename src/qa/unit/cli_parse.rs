//! Tokenizer tests for argv parsing.

use crate::cli::args::{parse_args_with, Mode};
use crate::cli::parse::{parse_argv, Token};

#[test]
fn empty_argv_yields_no_tokens() {
    let tokens = parse_argv(&[]);
    assert!(tokens.is_empty());
}

#[test]
fn long_flag_tokenized() {
    let tokens = parse_argv(&["--help".to_string()]);
    assert_eq!(tokens, vec![Token::Flag("--help".to_string())]);
}

#[test]
fn short_flag_tokenized() {
    let tokens = parse_argv(&["-h".to_string()]);
    assert_eq!(tokens, vec![Token::Flag("-h".to_string())]);
}

#[test]
fn flag_equals_value() {
    let tokens = parse_argv(&["--time=5".to_string()]);
    assert_eq!(tokens, vec![Token::WithValue {
        name: "--time".to_string(),
        value: "5".to_string(),
    }]);
}

#[test]
fn flag_with_separate_value() {
    let tokens = parse_argv(&["-t".to_string(), "5".to_string()]);
    assert_eq!(tokens, vec![Token::WithValue {
        name: "-t".to_string(),
        value: "5".to_string(),
    }]);
}

#[test]
fn positional_is_not_a_flag() {
    let tokens = parse_argv(&["hello".to_string()]);
    assert_eq!(tokens, vec![Token::Positional("hello".to_string())]);
}

#[test]
fn end_to_end_short_flag() {
    let a = parse_args_with(&["-s".to_string()]);
    assert_eq!(a.mode, Mode::XmlRpcServer);
}

#[test]
fn end_to_end_mixed() {
    let a = parse_args_with(&[
        "-w".to_string(),
        "-p".to_string(), "8080".to_string(),
        "-B".to_string(), "127.0.0.1".to_string(),
        "--debug".to_string(),
    ]);
    assert_eq!(a.mode, Mode::WebServer);
    assert_eq!(a.server_port, 8080);
    assert_eq!(a.bind_address, "127.0.0.1");
    assert!(a.debug);
}

#[test]
fn combined_short_flags_split() {
    // -ds means -d -s.
    let a = parse_args_with(&["-ds".to_string()]);
    assert!(a.debug);
    assert_eq!(a.mode, Mode::XmlRpcServer);
}
