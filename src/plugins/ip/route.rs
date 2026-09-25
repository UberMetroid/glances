//! IPv4 route-table + fib_trie parsing for the `ip` plugin.
//!
//! `/proc/net/route` gives interface ↔ subnet/gateway mappings;
//! `/proc/net/fib_trie` gives the addresses assigned to this host
//! (`/32 host LOCAL` annotations). Pairing the two lets every
//! published field describe the same interface.

/// Parse a `/proc/net/route`-style line and return the interface name
/// and gateway hex for the row whose Destination is `00000000`.
pub fn parse_route_line(line: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 { return None; }
    let iface = parts[0].to_string();
    let dest = parts[1];
    let gw = parts[2];
    if dest == "00000000" {
        return Some((iface, gw.to_string()));
    }
    None
}

/// Convert a little-endian hex IP (as found in /proc/net/route) to
/// dotted notation. E.g. "0103A8C0" → "192.168.3.1".
pub fn hex_to_ipv4(hex: &str) -> Option<String> {
    if hex.len() != 8 { return None; }
    let bytes: Vec<u8> = (0..4).map(|i| u8::from_str_radix(&hex[i*2..i*2+2], 16).ok()).collect::<Option<Vec<u8>>>()?;
    Some(format!("{}.{}.{}.{}", bytes[3], bytes[2], bytes[1], bytes[0]))
}

/// /proc/net/route hex field → u32. The file is little-endian:
/// "0103A8C0" → 192.168.3.1 → 0xC0A80301.
pub fn hex_le_to_u32(hex: &str) -> Option<u32> {
    if hex.len() != 8 { return None; }
    let b: Vec<u8> = (0..4)
        .map(|i| u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok())
        .collect::<Option<Vec<u8>>>()?;
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}

/// Dotted-quad → u32 ("192.168.3.1" → 0xC0A80301).
pub fn ipv4_to_u32(s: &str) -> Option<u32> {
    let mut out = [0u8; 4];
    for (i, p) in s.split('.').enumerate() {
        if i >= 4 { return None; }
        out[i] = p.parse().ok()?;
    }
    Some(u32::from_be_bytes(out))
}

/// Loose IPv4 syntax check (four u8 octets); returns the string.
pub fn parse_ipv4(s: &str) -> Option<String> {
    if s.split('.').count() != 4 { return None; }
    for p in s.split('.') {
        if p.parse::<u8>().is_err() { return None; }
    }
    Some(s.to_string())
}

/// Look up the default gateway from /proc/net/route. Returns `""` on
/// any error so callers can blindly insert the result.
pub fn default_gateway() -> String {
    let text = match std::fs::read_to_string("/proc/net/route") {
        Ok(t) => t,
        Err(_) => return String::new(),
    };
    for line in text.lines().skip(1) {
        if let Some((_, gw)) = parse_route_line(line)
            && let Some(ip) = hex_to_ipv4(&gw) {
                return ip;
            }
    }
    String::new()
}

/// One row of /proc/net/route with hex fields decoded to u32 IPs
/// (host order, matching `hex_to_ipv4`'s little-endian source).
pub struct RouteRow {
    pub iface: String,
    pub dest: u32,
    pub gateway: u32,
    pub mask: u32,
}

/// All IPv4 route rows; empty vec on read error.
pub fn routes() -> Vec<RouteRow> {
    let Ok(text) = std::fs::read_to_string("/proc/net/route") else { return Vec::new(); };
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            // Columns: Iface Destination Gateway Flags RefCnt Use Metric Mask ...
            if p.len() < 8 { return None; }
            Some(RouteRow {
                iface: p[0].to_string(),
                dest: hex_le_to_u32(p[1])?,
                gateway: hex_le_to_u32(p[2])?,
                mask: hex_le_to_u32(p[7])?,
            })
        })
        .collect()
}

/// Interface holding the real default route (dest 0 + non-zero
/// gateway). Falls back to the first non-loopback interface.
pub fn default_iface() -> String {
    let rs = routes();
    if let Some(r) = rs.iter().find(|r| r.dest == 0 && r.gateway != 0) {
        return r.iface.clone();
    }
    super::primary_interface()
}

/// The local IP belonging to `iface`'s subnet route — so `address`,
/// `mask`, `gateway` and `mac` all describe the same interface on
/// multi-homed hosts. Falls back to the first non-loopback local IP.
pub fn address_for_iface(iface: &str, routes: &[RouteRow], local_ips: &[String]) -> String {
    if let Some(r) = routes.iter().find(|r| {
        r.iface == iface && r.dest != 0 && r.mask != u32::MAX
    }) {
        for ip in local_ips {
            if !ip.starts_with("127.")
                && let Some(v) = ipv4_to_u32(ip)
                    && v & r.mask == r.dest { return ip.clone(); }
        }
    }
    local_ips.iter().find(|i| !i.starts_with("127.")).cloned().unwrap_or_default()
}

/// Read the IPv4 netmask of `iface`'s subnet route from
/// /proc/net/route (the Mask column, little-endian hex). Returns ""
/// when no subnet route exists for the interface.
pub fn mask_from_route(iface: &str) -> String {
    let text = match std::fs::read_to_string("/proc/net/route") {
        Ok(t) => t,
        Err(_) => return String::new(),
    };
    for line in text.lines().skip(1) {
        let p: Vec<&str> = line.split_whitespace().collect();
        if p.len() < 8 || p[0] != iface || p[1] == "00000000" || p[7] == "FFFFFFFF" {
            continue;
        }
        if let Some(m) = hex_to_ipv4(p[7]) {
            return m;
        }
    }
    String::new()
}

/// Collect every local IPv4 address from /proc/net/fib_trie.
///
/// fib_trie pairs each `|-- <ip>` node line with the annotation on the
/// line *below* it (`/32 host LOCAL`, `/24 universe UNICAST`, ...). A
/// `host LOCAL` annotation marks an address assigned to this host —
/// so we hold the last `|--` IP as pending and commit it only when the
/// immediately following annotation says `host LOCAL`.
pub fn local_ips_from_fib_trie() -> Vec<String> {
    match std::fs::read_to_string("/proc/net/fib_trie") {
        Ok(t) => parse_fib_trie(&t),
        Err(_) => Vec::new(),
    }
}

/// Pure parser over fib_trie text (exposed for fixture tests).
pub fn parse_fib_trie(text: &str) -> Vec<String> {
    let mut pending: Option<String> = None;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("|--") {
            let ip = rest.split_whitespace().next().unwrap_or("");
            pending = parse_ipv4(ip);
        } else if trimmed.contains("host LOCAL") {
            if let Some(ip) = pending.take() {
                out.push(ip);
            }
        } else {
            // Any other line (a new +-- prefix node or a different
            // annotation) ends the pending IP's association.
            pending = None;
        }
    }
    out
}
