//! Tests for Linux /sys/class/net reader.

use crate::platform::linux::sys_class_net;

#[test]
fn list_interfaces_returns_at_least_one_on_linux() {
    if !cfg!(target_os = "linux") { return; }
    let ifaces = sys_class_net::list_interfaces().expect("read /sys/class/net");
    assert!(!ifaces.is_empty(), "Linux machines have at least one interface (lo)");
    assert!(ifaces.iter().any(|n| n == "lo"), "expected loopback in list");
}

#[test]
fn read_meta_loopback_returns_some_state() {
    if !cfg!(target_os = "linux") { return; }
    let m = sys_class_net::read_meta("lo").expect("read loopback meta");
    // State can be "up", "unknown", "notpresent" — just verify we got SOMETHING.
    assert!(!m.operstate.is_empty(), "operstate should not be empty");
    // is_physical should be false for loopback.
    assert!(!m.is_physical, "loopback has no /device symlink");
}

#[test]
fn read_meta_handles_missing_interface() {
    // On a real Linux box, reading a non-existent iface returns Ok with
    // operstate="unknown" (because we default-fill). On other OSes the
    // /sys/class/net read_dir fails first.
    let r = sys_class_net::read_meta("definitely-not-a-real-iface-xyz");
    if cfg!(target_os = "linux") {
        // Either Ok with operstate="unknown" OR Err — both are valid graceful behavior.
        if let Ok(m) = r {
            assert_eq!(m.operstate, "unknown");
        }
    }
}
