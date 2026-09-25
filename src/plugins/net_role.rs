//! Network interface role classification.
//!
//! Pure helpers that tag an interface `loopback`, `tailscale`, `local`
//! (RFC 1918), or untagged (`""`). Kept apart from the reader so the
//! rules stay unit-testable.

/// Operstate → connected. Tunnels say "unknown" while working.
pub fn iface_is_up(operstate: &str) -> bool {
    matches!(operstate, "up" | "unknown")
}

/// Dashboard role tag. Name first, then the attributed addresses.
pub fn iface_role(name: &str, ips: &[String]) -> &'static str {
    if name == "lo" {
        return "loopback";
    }
    if name.contains("tailscale") {
        return "tailscale";
    }
    for ip in ips {
        if ip.starts_with("127.") {
            return "loopback";
        }
        if in_subnet(ip, "100.64.0.0", 10) {
            return "tailscale";
        }
        if in_subnet(ip, "10.0.0.0", 8)
            || in_subnet(ip, "172.16.0.0", 12)
            || in_subnet(ip, "192.168.0.0", 16) {
                return "local";
            }
    }
    ""
}

fn in_subnet(ip: &str, net: &str, prefix: u32) -> bool {
    match (super::ip::ipv4_to_u32(ip), super::ip::ipv4_to_u32(net)) {
        (Some(a), Some(n)) => prefix < 32 && a & (!0u32 << (32 - prefix)) == n,
        _ => false,
    }
}
