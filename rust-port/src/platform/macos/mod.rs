//! macOS platform primitives — M0 placeholder.
//!
//! M4 will add: sysctl, host_info, mach readers via libc FFI.

pub fn is_macos() -> bool { cfg!(target_os = "macos") }
