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
    let a = parse_args_with(&["-w".to_string()]);
    assert_eq!(a.mode, Mode::WebServer);
}

#[test]
fn removed_server_flags_are_ignored() {
    // -s/-p/--server/--browser died with XML-RPC: unknown flags are
    // ignored and never flip the mode from the default.
    for argv in [["-s"], ["--server"], ["--browser"], ["-p"]] {
        let owned: Vec<String> = argv.iter().map(|s| s.to_string()).collect();
        assert_eq!(parse_args_with(&owned).mode, Mode::Standalone);
    }
}

#[test]
fn end_to_end_mixed() {
    let a = parse_args_with(&[
        "-w".to_string(),
        "-B".to_string(), "127.0.0.1".to_string(),
        "--debug".to_string(),
    ]);
    assert_eq!(a.mode, Mode::WebServer);
    assert_eq!(a.bind_address, "127.0.0.1");
    assert!(a.debug);
}

#[test]
fn combined_short_flags_split() {
    // -dq means -d -q.
    let a = parse_args_with(&["-dq".to_string()]);
    assert!(a.debug);
    assert!(a.quiet);
}

#[test]
fn attached_value_short_option_is_not_exploded() {
    // Regression: `-clocalhost` used to emit Flag("-c"), Flag("-l"),
    // Flag("-o"), Flag("-c"), Flag("-a"), … — the tail chars were
    // dispatched as real flags, silently flipping modes.
    let tokens = parse_argv(&["-clocalhost".to_string()]);
    assert_eq!(tokens, vec![Token::WithValue {
        name: "-c".to_string(),
        value: "localhost".to_string(),
    }]);
    let a = parse_args_with(&["-clocalhost".to_string()]);
    assert_eq!(a.mode, Mode::Client);
    assert_eq!(a.client_host.as_deref(), Some("localhost"));
}

#[test]
fn attached_numeric_values() {
    for (arg, want) in [("-t5", "5"), ("-uadmin", "admin")] {
        let tokens = parse_argv(&[arg.to_string()]);
        assert_eq!(tokens.len(), 1, "{arg} must be a single WithValue");
        match &tokens[0] {
            Token::WithValue { value, .. } => assert_eq!(value, want),
            other => panic!("{arg} tokenized as {other:?}"),
        }
    }
    let a = parse_args_with(&["-t5".to_string()]);
    assert_eq!(a.refresh_time, 5.0);
    // `-t5` must NOT enable light mode via a stray `-5`.
    assert!(!a.light);
}

#[test]
fn cluster_ending_in_value_flag_consumes_next() {
    // `-dt 5` → -d + -t 5 (argparse semantics).
    let a = parse_args_with(&["-dt".to_string(), "5".to_string()]);
    assert!(a.debug);
    assert_eq!(a.refresh_time, 5.0);
}

#[test]
fn double_dash_terminates_options() {
    // Everything after `--` is positional — `-w` there must not switch modes.
    let a = parse_args_with(&["--".to_string(), "-w".to_string()]);
    assert_ne!(a.mode, Mode::WebServer);
    assert_eq!(a.mode, Mode::Standalone);
}
