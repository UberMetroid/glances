//! Unit tests for the TUI key parser (pure state machine, no TTY).

use crate::outputs::tui::keys::{feed, Key};

fn run_seq(bytes: &[u8]) -> Option<Key> {
    let mut state = Vec::new();
    let mut out = None;
    for b in bytes {
        if let Some(k) = feed(&mut state, *b) {
            out = Some(k);
        }
    }
    out
}

#[test]
fn quit_keys_resolve() {
    assert_eq!(run_seq(b"q"), Some(Key::Quit));
    assert_eq!(run_seq(b"Q"), Some(Key::Quit));
    assert_eq!(run_seq(&[0x03]), Some(Key::Quit));
}

#[test]
fn arrows_need_full_sequence() {
    let mut state = Vec::new();
    assert_eq!(feed(&mut state, 0x1b), None);
    assert_eq!(feed(&mut state, b'['), None);
    assert_eq!(feed(&mut state, b'A'), Some(Key::Up));
    assert!(state.is_empty(), "state must clear after resolve");
    assert_eq!(run_seq(&[0x1b, b'[', b'B']), Some(Key::Down));
    assert_eq!(run_seq(&[0x1b, b'O', b'D']), Some(Key::Left));
}

#[test]
fn toggle_keys_resolve() {
    assert_eq!(run_seq(b"1"), Some(Key::TogglePercpu));
    assert_eq!(run_seq(b"h"), Some(Key::ToggleHelp));
    assert_eq!(run_seq(b"?"), Some(Key::ToggleHelp));
}

#[test]
fn plain_bytes_pass_through() {
    assert_eq!(run_seq(b"x"), Some(Key::Other(b'x')));
}

#[test]
fn unknown_escape_is_dropped_not_sticky() {
    let mut state = Vec::new();
    assert_eq!(feed(&mut state, 0x1b), None);
    assert_eq!(feed(&mut state, b'Z'), None);
    // Parser recovered: a fresh key still resolves.
    assert_eq!(feed(&mut state, b'q'), Some(Key::Quit));
}
