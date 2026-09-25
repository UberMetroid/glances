//! /proc readers for the process list (pure file parsers).

use std::collections::HashMap;
use std::fs;
/// Parsed `/proc/<pid>/stat` identity + time fields:
/// `(comm, state, utime, stime, nice, threads, cpu_num, blkio_ticks)`.
pub type StatFields = (String, char, u64, u64, i64, u64, u64, u64);
pub fn parse_stat_fields(line: &str) -> Option<StatFields> {
    let sp = line.find(' ')?;
    let rest = &line[sp + 1..];
    let open = rest.find('(')?;
    let close = rest.rfind(')')?;
    if close <= open {
        return None;
    }
    let comm = rest[open + 1..close].to_string();
    let tail: Vec<&str> = rest[close + 1..].split_whitespace().collect();
    // Need indices through nice/threads (17); processor (36) and
    // delayacct_blkio_ticks (39, upstream cpu_times.iowait) are optional.
    if tail.len() < 18 {
        return None;
    }
    let state = tail[0].chars().next()?;
    let utime = tail[11].parse::<u64>().ok()?;
    let stime = tail[12].parse::<u64>().ok()?;
    let nice = tail[16].parse::<i64>().ok()?;
    let threads = tail[17].parse::<u64>().ok()?;
    let cpu_num = tail.get(36).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    let blkio_ticks = tail.get(39).and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    Some((comm, state, utime, stime, nice, threads, cpu_num, blkio_ticks))
}

/// Parse `/proc/<pid>/status`: (state_char, uid, gids real/eff/saved).
/// Missing file → None. Gids default to the uid when unreadable.
pub fn parse_status_file(pid: u32) -> Option<(char, u32, (u32, u32, u32))> {
    let text = fs::read_to_string(format!("/proc/{}/status", pid)).ok()?;
    parse_status_text(&text)
}

/// Fixture-testable half of `parse_status_file`.
pub fn parse_status_text(text: &str) -> Option<(char, u32, (u32, u32, u32))> {
    let mut state = None;
    let mut uid = None;
    let mut gids = None;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("State:") {
            state = rest.trim().chars().next();
        } else if let Some(rest) = line.strip_prefix("Uid:") {
            uid = rest.split_whitespace().next()?.parse::<u32>().ok();
        } else if let Some(rest) = line.strip_prefix("Gid:") {
            let mut it = rest.split_whitespace().filter_map(|s| s.parse::<u32>().ok());
            gids = Some((it.next()?, it.next()?, it.next()?));
        }
        if state.is_some() && uid.is_some() && gids.is_some() {
            break;
        }
    }
    let uid = uid?;
    let gids = gids.unwrap_or((uid, uid, uid));
    Some((state?, uid, gids))
}

/// Parse `/proc/<pid>/statm`: (vms, rss, shared, text, lib, data, dirty)
/// in bytes (upstream `memory_info` parity).
pub fn parse_statm(pid: u32, page: u64) -> Option<(u64, u64, u64, u64, u64, u64, u64)> {
    let text = fs::read_to_string(format!("/proc/{}/statm", pid)).ok()?;
    parse_statm_text(&text, page)
}

/// Fixture-testable half of `parse_statm`.
pub fn parse_statm_text(text: &str, page: u64) -> Option<(u64, u64, u64, u64, u64, u64, u64)> {
    let mut it = text.split_whitespace();
    let n = |it: &mut std::str::SplitWhitespace<'_>| {
        it.next()?.parse::<u64>().ok().map(|v| v.saturating_mul(page))
    };
    Some((n(&mut it)?, n(&mut it)?, n(&mut it)?, n(&mut it)?, n(&mut it)?, n(&mut it)?, n(&mut it)?))
}

/// Parse `/proc/<pid>/io`: (read_bytes, write_bytes, read_count=syscr,
/// write_count=syscw) — upstream `io_counters` parity. Missing → zeros.
pub fn parse_io(pid: u32) -> (u64, u64, u64, u64) {
    match fs::read_to_string(format!("/proc/{}/io", pid)) {
        Ok(t) => parse_io_text(&t),
        Err(_) => (0, 0, 0, 0),
    }
}

/// Fixture-testable half of `parse_io`.
pub fn parse_io_text(text: &str) -> (u64, u64, u64, u64) {
    let mut read = 0;
    let mut write = 0;
    let mut rcount = 0;
    let mut wcount = 0;
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("read_bytes:") {
            read = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("write_bytes:") {
            write = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("syscr:") {
            rcount = rest.trim().parse().unwrap_or(0);
        } else if let Some(rest) = line.strip_prefix("syscw:") {
            wcount = rest.trim().parse().unwrap_or(0);
        }
    }
    (read, write, rcount, wcount)
}

/// Read `/proc/<pid>/cmdline` (NUL-separated) joined with spaces.
/// Empty (kernel threads) → empty string.
/// Read `/proc/<pid>/cmdline` as an argv list (upstream parity:
/// `cmdline` is a list, not a joined string).
pub fn read_cmdline(pid: u32) -> Vec<String> {
    match fs::read(format!("/proc/{}/cmdline", pid)) {
        Ok(bytes) => bytes
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect(),
        Err(_) => Vec::new(),
    }
}

/// Map uid → username via `/etc/passwd`. Unknown → numeric id string.
pub fn build_user_map() -> HashMap<u32, String> {
    let mut map = HashMap::new();
    let text = match fs::read_to_string("/etc/passwd") {
        Ok(t) => t,
        Err(_) => return map,
    };
    for line in text.lines() {
        let mut parts = line.split(':');
        let name = match parts.next() {
            Some(n) => n,
            None => continue,
        };
        let _ = parts.next();
        if let Some(uid) = parts.next().and_then(|s| s.parse::<u32>().ok()) {
            map.entry(uid).or_insert_with(|| name.to_string());
        }
    }
    map
}

/// Sum the aggregate `cpu ` line in `/proc/stat` (all counters).
pub fn read_total_cpu() -> u64 {
    let text = match fs::read_to_string("/proc/stat") {
        Ok(t) => t,
        Err(_) => return 0,
    };
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("cpu ") {
            return rest
                .split_whitespace()
                .filter_map(|f| f.parse::<u64>().ok())
                .fold(0u64, |a, b| a.saturating_add(b));
        }
    }
    0
}
