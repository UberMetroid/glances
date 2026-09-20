//! Terminal I/O primitives for the interactive UI: raw mode, window
//! size, and polled stdin reads. All `unsafe` lives here under
//! `src/platform` per AC-11; callers use the safe wrappers.
//!
//! ABI notes (Linux/glibc/musl, x86_64 + aarch64): `termios` is 60
//! bytes, `TIOCGWINSZ` is 0x5413, `POLLIN` is 0x1.

use std::io::Read;

/// Linux `termios`: 4×u32 flags, line byte, 32 control chars, speeds.
#[repr(C)]
struct Termios {
    c_iflag: u32,
    c_oflag: u32,
    c_cflag: u32,
    c_lflag: u32,
    c_line: u8,
    c_cc: [u8; 32],
    c_ispeed: u32,
    c_ospeed: u32,
}

const _: () = assert!(std::mem::size_of::<Termios>() == 60);

const VMIN: usize = 6;
const VTIME: usize = 5;

const BRKINT: u32 = 0x2;
const ICRNL: u32 = 0x100;
const INPCK: u32 = 0x10;
const ISTRIP: u32 = 0x20;
const IXON: u32 = 0x400;
const OPOST: u32 = 0x1;
const CS8: u32 = 0x30;
const ECHO: u32 = 0x8;
const ICANON: u32 = 0x2;
const IEXTEN: u32 = 0x8000;
const ISIG: u32 = 0x1;

/// `winsize` for `TIOCGWINSZ`.
#[repr(C)]
struct Winsize {
    ws_row: u16,
    ws_col: u16,
    _xpixel: u16,
    _ypixel: u16,
}

#[repr(C)]
struct PollFd {
    fd: i32,
    events: i16,
    revents: i16,
}

unsafe extern "C" {
    fn tcgetattr(fd: i32, termios: *mut Termios) -> i32;
    fn tcsetattr(fd: i32, action: i32, termios: *const Termios) -> i32;
    fn ioctl(fd: i32, request: u64, ...) -> i32;
    fn poll(fds: *mut PollFd, nfds: u64, timeout: i32) -> i32;
}

const STDIN_FD: i32 = 0;
const TCSANOW: i32 = 0;
const TIOCGWINSZ: u64 = 0x5413;
const POLLIN: i16 = 0x1;

fn apply(raw: &Termios) -> std::io::Result<()> {
    if unsafe { tcsetattr(STDIN_FD, TCSANOW, raw) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Guard that puts stdin into raw mode on creation and restores the
/// original mode on drop (including on panic unwind).
pub struct RawMode {
    orig: Termios,
}

impl RawMode {
    pub fn enter() -> std::io::Result<Self> {
        let mut orig: Termios = unsafe { std::mem::zeroed() };
        if unsafe { tcgetattr(STDIN_FD, &mut orig) } != 0 {
            return Err(std::io::Error::last_os_error());
        }
        let mut raw: Termios = unsafe { std::ptr::read(&orig) };
        raw.c_iflag &= !(BRKINT | ICRNL | INPCK | ISTRIP | IXON);
        raw.c_oflag &= !OPOST;
        raw.c_cflag |= CS8;
        // No echo, no canonical mode, no signals (Ctrl-C arrives as 0x03
        // and quits cleanly through the key parser instead of killing
        // the process with the terminal left raw).
        raw.c_lflag &= !(ECHO | ICANON | IEXTEN | ISIG);
        raw.c_cc[VMIN] = 1;
        raw.c_cc[VTIME] = 0;
        apply(&raw)?;
        Ok(Self { orig })
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = apply(&self.orig);
    }
}

/// Current terminal size in (rows, cols). Falls back to 24×80 when the
/// ioctl fails (piped output, dumb terminals).
pub fn size() -> (u16, u16) {
    let mut ws = Winsize { ws_row: 0, ws_col: 0, _xpixel: 0, _ypixel: 0 };
    let rc = unsafe { ioctl(STDIN_FD, TIOCGWINSZ, &mut ws) };
    if rc == 0 && ws.ws_row > 0 && ws.ws_col > 0 {
        (ws.ws_row, ws.ws_col)
    } else {
        (24, 80)
    }
}

/// Wait up to `timeout_ms` for stdin readability.
pub fn wait_stdin(timeout_ms: i32) -> std::io::Result<bool> {
    let mut pfd = PollFd { fd: STDIN_FD, events: POLLIN, revents: 0 };
    let rc = unsafe { poll(&mut pfd, 1, timeout_ms) };
    if rc < 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(rc > 0)
    }
}

/// Read one stdin byte (caller should `wait_stdin` first for timeouts).
pub fn read_stdin_byte() -> std::io::Result<u8> {
    let mut buf = [0u8; 1];
    std::io::stdin().read_exact(&mut buf)?;
    Ok(buf[0])
}
