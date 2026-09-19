//! Linux-only platform primitives (M3).
//!
//! Every reader here parses a `/proc` or `/sys` text file. The crate is
//! intentionally not cross-platform — `macos` and `windows` platforms
//! were removed in v0.9.0 because the maintainer does not have hardware
//! to test FFI on, and shipping stub implementations that return errors
//! at runtime is misleading. If you want macOS or Windows support,
//! open an issue with a hardware-donation offer.
//!
//! Each module exposes a `read()` function that parses the corresponding
//! `/proc` or `/sys` file, plus a `parse(text)` function for testing with
//! captured fixtures.

pub mod linux;

#[cfg(not(target_os = "linux"))]
compile_error!(
    "glances-rs is Linux-only. Cross-compiling to other targets is not supported. \
     See README.md and docs/limitations.md for the rationale."
);

/// Hard assertion at crate load time: every binary must be running on
/// Linux. We do this in `run()` rather than at compile time so cross-
/// compiling the lib for unit testing still works, but running the
/// binary on a non-Linux host fails fast.
pub fn assert_linux_host() {
    if cfg!(not(target_os = "linux")) {
        panic!("glances-rs only runs on Linux (compiled for {}).", std::env::consts::OS);
    }
}
