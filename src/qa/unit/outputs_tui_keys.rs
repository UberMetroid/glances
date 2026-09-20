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
    // `q` passes through (quit is decided by the dispatcher, and `q`
    // is editable text in the filter prompt); ESC quits, Ctrl-C
    // interrupts.
    assert_eq!(run_seq(b"q"), Some(Key::Byte(b'q')));
    assert_eq!(run_seq(b"Q"), Some(Key::Byte(b'Q')));
    assert_eq!(run_seq(&[0x03]), Some(Key::Interrupt));
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
    assert_eq!(run_seq(b"1"), Some(Key::Byte(b'1')));
    assert_eq!(run_seq(b"h"), Some(Key::Byte(b'h')));
    assert_eq!(run_seq(b"?"), Some(Key::Byte(b'?')));
    assert_eq!(run_seq(b"\r"), Some(Key::Enter));
    assert_eq!(run_seq(&[0x12]), Some(Key::Refresh));
    assert_eq!(run_seq(&[0x1b, b'[', b'1', b'5', b'~']), Some(Key::Refresh));
}

#[test]
fn shift_arrows_resolve() {
    assert_eq!(run_seq(&[0x1b, b'[', b'1', b';', b'2', b'D']), Some(Key::ShiftLeft));
    assert_eq!(run_seq(&[0x1b, b'[', b'1', b';', b'2', b'C']), Some(Key::ShiftRight));
}

#[test]
fn plain_bytes_pass_through() {
    assert_eq!(run_seq(b"x"), Some(Key::Byte(b'x')));
}

#[test]
fn unknown_escape_is_dropped_not_sticky() {
    let mut state = Vec::new();
    assert_eq!(feed(&mut state, 0x1b), None);
    assert_eq!(feed(&mut state, b'Z'), None);
    // Parser recovered: a fresh key still resolves.
    assert_eq!(feed(&mut state, b'q'), Some(Key::Byte(b'q')));
}
