//! Tests for Linux /proc/uptime + /proc/stat::btime.

use crate::platform::linux::proc_uptime;

#[test]
fn uptime_parses_fractional() {
    use std::time::Duration;
    let d = proc_uptime::uptime_duration().unwrap_or(Duration::from_secs(0));
    // Just verify it's nonzero on a real machine; on other OSes we get 0.
    if cfg!(target_os = "linux") { assert!(d.as_secs() > 0, "uptime should be > 0 on Linux"); }
}

#[test]
fn read_boot_time_returns_system_time() {
    use std::time::{Duration, UNIX_EPOCH};
    let bt = proc_uptime::read_boot_time().unwrap_or(UNIX_EPOCH);
    if cfg!(target_os = "linux") {
        assert!(bt > UNIX_EPOCH, "boot time should be after epoch");
        // Should be in the past — within ~50 years of now.
        let now = std::time::SystemTime::now();
        let diff = now.duration_since(bt).unwrap_or(Duration::from_secs(0));
        assert!(diff.as_secs() > 0 && diff.as_secs() < 50 * 365 * 24 * 3600,
                "boot time out of plausible range");
    }
}
