//! OS-specific metric collection. Cross-platform plugins reach into here
//! using `#[cfg(target_os = "...")]` to pick the right implementation.
//!
//! M0: empty module structure only. M3/M4/M5 add Linux/macOS/Windows
//! primitive readers respectively.

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;
