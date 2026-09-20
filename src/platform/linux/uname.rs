//! `uname(2)` FFI — sysname/release/machine for the system plugin.
//! Unsafe is contained here per the AC-11 platform-layer allowlist.
//!
//! glibc `struct utsname` is six 65-byte char arrays (sysname, nodename,
//! release, version, machine, domainname). The buffer is written fully
//! by the kernel, so the declaration must be complete.

use std::ffi::CStr;
use std::os::raw::c_char;

const UTS_LEN: usize = 65;

#[repr(C)]
struct Utsname {
    sysname: [c_char; UTS_LEN],
    nodename: [c_char; UTS_LEN],
    release: [c_char; UTS_LEN],
    version: [c_char; UTS_LEN],
    machine: [c_char; UTS_LEN],
    domainname: [c_char; UTS_LEN],
}

const _: () = assert!(std::mem::size_of::<Utsname>() == 6 * UTS_LEN);

unsafe extern "C" {
    fn uname(buf: *mut Utsname) -> i32;
}

/// Kernel identity fields, all NUL-trimmed.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct UnameInfo {
    /// "Linux"
    pub sysname: String,
    /// Hostname as the kernel sees it.
    pub nodename: String,
    /// Kernel release, e.g. "6.8.0-49-generic".
    pub release: String,
    /// Kernel build version, e.g. "#49-Ubuntu SMP ...".
    pub version: String,
    /// Hardware machine, e.g. "x86_64".
    pub machine: String,
}

fn read_field(f: &[c_char; UTS_LEN]) -> String {
    unsafe { CStr::from_ptr(f.as_ptr()) }.to_string_lossy().into_owned()
}

/// `uname(2)` wrapper. Returns `None` on the (practically impossible)
/// syscall failure path so callers can keep a sane fallback.
pub fn uname_info() -> Option<UnameInfo> {
    let mut buf = Utsname {
        sysname: [0; UTS_LEN],
        nodename: [0; UTS_LEN],
        release: [0; UTS_LEN],
        version: [0; UTS_LEN],
        machine: [0; UTS_LEN],
        domainname: [0; UTS_LEN],
    };
    let rc = unsafe { uname(&mut buf) };
    if rc != 0 {
        return None;
    }
    Some(UnameInfo {
        sysname: read_field(&buf.sysname),
        nodename: read_field(&buf.nodename),
        release: read_field(&buf.release),
        version: read_field(&buf.version),
        machine: read_field(&buf.machine),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uname_reports_linux() {
        let u = uname_info().expect("uname must succeed on Linux");
        assert_eq!(u.sysname, "Linux");
        assert!(!u.release.is_empty());
        assert!(!u.machine.is_empty());
    }
}
