//! Linux platform primitives (M3). All readers are std-only — no libc FFI.
//!
//! Each module exposes a `read()` function that parses the corresponding
//! /proc or /sys file, plus a `parse(text)` function for testing with
//! captured fixtures.

pub mod proc_stat;
pub mod proc_meminfo;
pub mod proc_loadavg;
pub mod proc_uptime;
pub mod proc_net_dev;
pub mod proc_diskstats;
pub mod sys_class_net;
pub mod sys_class_hwmon;

pub fn is_linux() -> bool { cfg!(target_os = "linux") }
