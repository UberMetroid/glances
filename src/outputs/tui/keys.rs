//! Keyboard input for the TUI: escape-sequence parser plus a
//! poll-based byte reader so a lone `ESC` doesn't block the loop.
//!
//! The parser only distinguishes structural keys (arrows, Enter,
//! F5, quit); every other byte passes through as `Key::Byte` and is
//! dispatched against the upstream hotkey table in `tui/actions`.

use std::time::Duration;

/// Parsed key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    /// ESC: quit, or cancel an edit/confirm in progress.
    Quit,
    /// Ctrl-C: always quit.
    Interrupt,
    Up,
    Down,
    Left,
    Right,
    ShiftLeft,
    ShiftRight,
    Enter,
    Refresh,
    Byte(u8),
}

/// Feed one byte into the escape-sequence state machine.
/// Returns a finished `Key` once enough bytes arrived, or `None` to
/// keep feeding. `ESC` alone resolves on stream end (see `read_key`).
/// The state is cleared whenever a key resolves.
pub fn feed(state: &mut Vec<u8>, b: u8) -> Option<Key> {
    state.push(b);
    let key = match state.as_slice() {
        [0x03] => Some(Key::Interrupt), // Ctrl-C (ISIG is off in raw mode)
        [0x1b] => None, // wait: ESC alone or sequence start
        [0x1b, b'[', b'A'] | [0x1b, b'O', b'A'] => Some(Key::Up),
        [0x1b, b'[', b'B'] | [0x1b, b'O', b'B'] => Some(Key::Down),
        [0x1b, b'[', b'C'] | [0x1b, b'O', b'C'] => Some(Key::Right),
        [0x1b, b'[', b'D'] | [0x1b, b'O', b'D'] => Some(Key::Left),
        [0x1b, b'[', b'1', b';', b'2', b'D'] => Some(Key::ShiftLeft),
        [0x1b, b'[', b'1', b';', b'2', b'C'] => Some(Key::ShiftRight),
        [0x1b, b'[', b'1', b'5', b'~'] => Some(Key::Refresh), // F5
        [0x0d] => Some(Key::Enter),
        [0x12] => Some(Key::Refresh), // Ctrl-R (upstream KEY_F5/18)
        // `q` passes through: it quits at top level but is editable
        // text while the filter prompt is open.
        [single] if state.len() == 1 => Some(Key::Byte(*single)),
        // Unknown or overlong escape sequence: drop it (upstream ignores
        // unknown keys; a lone ESC is resolved by the read_key timeout).
        _ => None,
    };
    // Drop overlong garbage so one bad sequence can't wedge the parser.
    if key.is_some() || state.len() > 8 {
        state.clear();
    }
    // Unknown 2-byte sequences (e.g. ESC + letter) must not linger: if no
    // key resolved and this isn't a viable prefix, reset and move on.
    if key.is_none() && !is_prefix(state) {
        state.clear();
    }
    key
}

/// True while `state` could still grow into a known sequence.
fn is_prefix(state: &[u8]) -> bool {
    matches!(
        state,
        [0x1b] | [0x1b, b'[']
            | [0x1b, b'O']
            | [0x1b, b'[', b'1']
            | [0x1b, b'[', b'1', b'5']
            | [0x1b, b'[', b'1', b';']
            | [0x1b, b'[', b'1', b';', b'2']
    )
}

/// Wait up to `timeout` for one stdin byte. `Ok(None)` on timeout.
/// Polling lives in `platform::linux::tty` (raw `unsafe` per AC-11).
pub fn read_byte(timeout: Duration) -> std::io::Result<Option<u8>> {
    use crate::platform::linux::tty;
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    if !tty::wait_stdin(ms)? {
        return Ok(None);
    }
    Ok(Some(tty::read_stdin_byte()?))
}

/// Read one key, waiting at most `timeout` total. A lone ESC resolves to
/// Quit once the timeout expires with no follow-up bytes.
pub fn read_key(timeout: Duration) -> Option<Key> {
    let deadline = std::time::Instant::now() + timeout;
    let mut state = Vec::new();
    loop {
        let now = std::time::Instant::now();
        if now >= deadline && state.is_empty() {
            return None;
        }
        let wait = deadline.saturating_duration_since(now);
        match read_byte(wait).ok()? {
            Some(b) => {
                if let Some(k) = feed(&mut state, b) {
                    return Some(k);
                }
            }
            None => {
                // Timeout: lone ESC quits, anything else is dropped.
                if state == vec![0x1b] {
                    return Some(Key::Quit);
                }
                return None;
            }
        }
    }
}
