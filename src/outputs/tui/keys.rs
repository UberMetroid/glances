//! Keyboard input for the TUI: escape-sequence parser plus a
//! poll-based byte reader so a lone `ESC` doesn't block the loop.
//!
//! Handled keys (upstream curses parity subset): `q`/`Q`/ESC/Ctrl-C
//! quit, arrows move the process cursor, `1` toggles per-CPU, `h`
//! toggles the help overlay. Everything else is ignored.

use std::io::Read;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

/// Parsed key press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Quit,
    Up,
    Down,
    Left,
    Right,
    TogglePercpu,
    ToggleHelp,
    Other(u8),
}

/// Feed one byte into the escape-sequence state machine.
/// Returns a finished `Key` once enough bytes arrived, or `None` to
/// keep feeding. `ESC` alone resolves on stream end (see `read_key`).
/// The state is cleared whenever a key resolves.
pub fn feed(state: &mut Vec<u8>, b: u8) -> Option<Key> {
    state.push(b);
    let key = match state.as_slice() {
        [0x03] => Some(Key::Quit), // Ctrl-C (ISIG is off in raw mode)
        [b'q'] | [b'Q'] => Some(Key::Quit),
        [0x1b] => None, // wait: ESC alone or sequence start
        [0x1b, b'[', b'A'] | [0x1b, b'O', b'A'] => Some(Key::Up),
        [0x1b, b'[', b'B'] | [0x1b, b'O', b'B'] => Some(Key::Down),
        [0x1b, b'[', b'C'] | [0x1b, b'O', b'C'] => Some(Key::Right),
        [0x1b, b'[', b'D'] | [0x1b, b'O', b'D'] => Some(Key::Left),
        [b'1'] => Some(Key::TogglePercpu),
        [b'h'] | [b'H'] | [b'?'] => Some(Key::ToggleHelp),
        [single] if state.len() == 1 => Some(Key::Other(*single)),
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
    matches!(state, [0x1b] | [0x1b, b'['] | [0x1b, b'O'])
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

extern "C" {
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
}

const POLLIN: i16 = 0x1;

/// Wait up to `timeout` for one stdin byte. `Ok(None)` on timeout.
pub fn read_byte(timeout: Duration) -> std::io::Result<Option<u8>> {
    let mut pfd = PollFd { fd: std::io::stdin().as_raw_fd(), events: POLLIN, revents: 0 };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    let rc = unsafe { poll(&mut pfd, 1, ms) };
    if rc < 0 {
        return Err(std::io::Error::last_os_error());
    }
    if rc == 0 {
        return Ok(None);
    }
    let mut buf = [0u8; 1];
    std::io::stdin().read_exact(&mut buf)?;
    Ok(Some(buf[0]))
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
