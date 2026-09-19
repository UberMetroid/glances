//! Windows platform primitives — M0 placeholder.
//!
//! M5 will add: GetSystemTimes, PDH, IPHLPAPI FFI declarations.

pub fn is_windows() -> bool { cfg!(target_os = "windows") }
