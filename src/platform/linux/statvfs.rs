//! `statvfs` FFI binding + safe wrapper. Used by the fs plugin to compute
//! per-mount filesystem usage. Unsafe is contained here (in the platform
//! layer, which the unsafe-allowlist permits) so the plugin code stays
//! pure safe Rust.
//!
//! The C signature is `int statvfs(const char *path, struct statvfs *buf)`.
//! We must declare the COMPLETE libc `struct statvfs` — the callee writes
//! every field including the trailing `__f_spare` array, so an undersized
//! declaration is a stack buffer overflow, not just a missing field.
//! The layout below matches glibc on x86_64 / aarch64 (120 bytes); musl's
//! variant is smaller (112 bytes) and also fits inside this buffer, with
//! the fields we actually read at identical offsets.

use std::ffi::CString;
use std::os::raw::c_char;

use crate::core::error::{GlancesError, Result};

#[repr(C)]
#[derive(Default)]
struct Statvfs {
    f_bsize: u64,
    f_frsize: u64,
    f_blocks: u64,
    f_bfree: u64,
    f_bavail: u64,
    f_files: u64,
    f_ffree: u64,
    f_favail: u64,
    f_fsid: u64,
    /// glibc `int __f_unused` — present on x86_64/aarch64 so `f_flag` sits
    /// at offset 80, not 72. Keeps the buffer layout ABI-correct even
    /// though we never read this field.
    f_unused: i32,
    f_flag: u64,
    f_namemax: u64,
    /// glibc `int __f_spare[6]` — the libc call zeroes this tail; the
    /// buffer must include it or those 24 bytes land on the stack.
    f_spare: [i32; 6],
}

// glibc/musl `struct statvfs` on 64-bit Linux is 112–120 bytes; ours is 120.
const _: () = assert!(std::mem::size_of::<Statvfs>() == 120);

extern "C" {
    fn statvfs(path: *const c_char, buf: *mut Statvfs) -> i32;
}

/// Filesystem usage statistics for a single mount point.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FsUsage {
    /// Block size (fs block size, in bytes).
    pub bsize: u64,
    /// Total blocks * fragment size.
    pub total: u64,
    /// Free blocks * fragment size.
    pub free: u64,
    /// Available blocks * fragment size (root-reserved subtracted).
    pub avail: u64,
    /// Used = total - free.
    pub used: u64,
    /// Percent used (0..=100).
    pub percent: f64,
}

/// `statvfs(3)` wrapper. Returns an `Err` if the path can't be queried
/// (e.g. path doesn't exist, permission denied, or filesystem was
/// unmounted between scanning `/proc/mounts` and calling statvfs).
pub fn statvfs_path(path: &str) -> Result<FsUsage> {
    let c_path = CString::new(path).map_err(|e| GlancesError::Parse(e.to_string()))?;
    let mut buf = Statvfs::default();
    let rc = unsafe { statvfs(c_path.as_ptr(), &mut buf) };
    if rc != 0 {
        return Err(GlancesError::Other(format!("statvfs({}) failed", path)));
    }
    // Compute byte totals from fragment size × block count.
    let frsize = buf.f_frsize;
    let total = buf.f_blocks.saturating_mul(frsize);
    let free = buf.f_bfree.saturating_mul(frsize);
    let avail = buf.f_bavail.saturating_mul(frsize);
    let used = total.saturating_sub(free);
    let percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    Ok(FsUsage {
        bsize: buf.f_bsize,
        total,
        free,
        avail,
        used,
        percent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn struct_layout_matches_libc_abi() {
        // Offsets follow glibc bits/statvfs.h on x86_64/aarch64.
        assert_eq!(std::mem::size_of::<Statvfs>(), 120);
        assert_eq!(std::mem::offset_of!(Statvfs, f_bsize), 0);
        assert_eq!(std::mem::offset_of!(Statvfs, f_fsid), 64);
        assert_eq!(std::mem::offset_of!(Statvfs, f_flag), 80);
        assert_eq!(std::mem::offset_of!(Statvfs, f_namemax), 88);
        assert_eq!(std::mem::offset_of!(Statvfs, f_spare), 96);
    }

    #[test]
    fn statvfs_does_not_write_past_struct() {
        // Canary: call the raw FFI into a padded buffer and verify nothing
        // past offset 120 is touched.
        extern "C" { fn statvfs(path: *const c_char, buf: *mut u8) -> i32; }
        let c = CString::new("/").unwrap();
        let mut raw = [0xAAu8; 256];
        let rc = unsafe { statvfs(c.as_ptr(), raw.as_mut_ptr()) };
        assert_eq!(rc, 0);
        let touched_beyond = raw[120..].iter().position(|b| *b != 0xAA);
        assert_eq!(touched_beyond, None, "statvfs wrote past byte 120");
    }

    #[test]
    fn root_fs_reports_sane_values() {
        let u = statvfs_path("/").unwrap();
        assert!(u.total > 0);
        assert!(u.used <= u.total);
        assert!(u.percent >= 0.0 && u.percent <= 100.0);
    }
}