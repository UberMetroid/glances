//! `statvfs` FFI binding + safe wrapper. Used by the fs plugin to compute
//! per-mount filesystem usage. Unsafe is contained here (in the platform
//! layer, which the unsafe-allowlist permits) so the plugin code stays
//! pure safe Rust.
//!
//! The C signature is `int statvfs(const char *path, struct statvfs *buf)`;
//! we declare only the fields we actually read, in the order the kernel
//! expects them. Field widths are governed by the libc `statvfs.h` layout
//! for the target architecture — Linux x86_64 / aarch64 / etc. all match
//! the layout below for the standard 64-bit `fsblkcnt_t` types.

use std::ffi::CString;
use std::os::raw::c_char;

use crate::core::error::{GlancesError, Result};

#[repr(C)]
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
    f_flag: u64,
    f_namemax: u64,
}

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
    let mut buf = Statvfs {
        f_bsize: 0,
        f_frsize: 0,
        f_blocks: 0,
        f_bfree: 0,
        f_bavail: 0,
        f_files: 0,
        f_ffree: 0,
        f_favail: 0,
        f_fsid: 0,
        f_flag: 0,
        f_namemax: 0,
    };
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