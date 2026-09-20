//! RAID plugin — Linux MDRAID arrays from /proc/mdstat.
//!
//! Format example (one `mdN` block per array):
//!
//! ```text
//! md0 : active raid1 sda1[0] sdb1[1]
//!       1953511936 blocks super 1.2 [2/2] [UU]
//!
//!       [====>................]  recovery = 20.0% (390000000/1953511936) finish=10.0min speed=120000K/sec
//!
//! md1 : active raid5 sdc1[0] sdd1[1] sde1[2]
//!       3907024128 blocks super 1.2 level 5 [3/3] [UUU]
//! ```
//!
//! Status tokens: `active`, `inactive`, `degraded` (keyword); rebuild
//! progress may be appended (`resync`, `recover`, `reshape`). Component
//! states `[UU]` use letters per slot: `U`=up, `_`=down/removed, `F`=faulty.

use std::collections::BTreeMap;
use std::fs;

use crate::core::error::Result;
use crate::core::plugin::{GlancesPluginModel, Plugin};
use crate::core::value::Value;

pub const NAME: &str = "raid";

pub fn register(stats: &crate::core::stats::GlancesStats) {
    stats.register(Box::new(RaidPlugin::new()));
}

#[derive(Debug, Clone, PartialEq)]
pub struct MdEntry {
    pub name: String,
    pub status: String,
    pub level: String,
    pub total_devices: u32,
    pub working_devices: u32,
    pub failed_devices: u32,
    pub components: Vec<String>,
}

/// Parse one `mdN` header line, e.g.
/// `md0 : active raid1 sda1[0] sdb1[2]` → returns the struct with
/// components filled but totals at 0 (they live on the next line).
pub fn parse_header(line: &str) -> Option<MdEntry> {
    // Expect "<name> : <status_word(s)> <level_word> <component>..."
    let mut parts = line.split_whitespace();
    let name = parts.next()?.to_string();
    if !name.starts_with("md") { return None; }
    let sep = parts.next()?;
    if sep != ":" { return None; }
    // Status word(s): typically one ("active"), but kernels also emit
    // "active (read-only)" / "inactive". Collect everything until we hit
    // the raid-level token (starts with "raid" + digit, optional suffix).
    let mut status_tokens: Vec<&str> = Vec::new();
    let mut level = String::new();
    let mut components: Vec<String> = Vec::new();
    loop {
        let tok = match parts.next() {
            Some(t) => t,
            None => break,
        };
        if tok.starts_with("raid") && tok.chars().nth(4).map_or(false, |c| c.is_ascii_digit()) {
            level = tok.to_string();
            // Everything after the level is component names like sda1[0].
            for c in parts {
                components.push(c.to_string());
            }
            break;
        } else {
            status_tokens.push(tok);
        }
    }
    let status = status_tokens.join(" ");
    Some(MdEntry {
        name,
        status: if status.is_empty() { "unknown".to_string() } else { status },
        level,
        total_devices: 0,
        working_devices: 0,
        failed_devices: 0,
        components,
    })
}

/// Parse the totals line, e.g.
/// `1953511936 blocks super 1.2 [2/2] [UU]`.
/// We extract the `[N/M]` and `[state]` brackets; everything else is
/// rebuilt-config metadata we ignore.
pub fn parse_status_line(line: &str) -> Option<(u32, u32)> {
    // Find "[<num>/<num>]" — total / working device count.
    let open = line.find('[')?;
    let mid = line[open + 1..].find('/')?;
    let close = line[open + 1 + mid + 1..].find(']')?;
    let total: u32 = line[open + 1..open + 1 + mid].parse().ok()?;
    let working: u32 = line[open + 1 + mid + 1..open + 1 + mid + 1 + close].parse().ok()?;
    Some((total, working))
}

/// Count failed components from the state bracket, e.g. `[UU_]` → 1 failed.
pub fn count_failed_from_state(line: &str) -> u32 {
    // Find the LAST "[...]" — the first is "[N/M]", the last is state.
    let bytes = line.as_bytes();
    let mut start = None;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'[' { start = Some(i); }
    }
    let Some(start) = start else { return 0 };
    let end = match line[start..].find(']') {
        Some(e) => start + e,
        None => return 0,
    };
    line[start + 1..end].chars().filter(|c| *c != 'U').count() as u32
}

/// Parse the whole /proc/mdstat text into MdEntry records.
pub fn parse_mdstat(text: &str) -> Vec<MdEntry> {
    let mut out: Vec<MdEntry> = Vec::new();
    let mut current: Option<MdEntry> = None;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            // Blank line ends an md block.
            if let Some(e) = current.take() {
                out.push(e);
            }
            continue;
        }
        // Personalities / unused devices headers are not md blocks.
        if trimmed.starts_with("Personalities") { continue; }
        if trimmed.starts_with("unused") { continue; }
        if let Some(e) = parse_header(trimmed) {
            current = Some(e);
            continue;
        }
        if let Some(cur) = current.as_mut() {
            // The `[N/M]` count and `[UU]` state bracket live together on
            // the "blocks" line. Progress lines (`[====>...] resync`) and
            // bitmap lines (`[0KB]`) also contain brackets — counting
            // state chars there would fabricate failed devices, so only
            // parse state on the line where the count bracket matched.
            if let Some((total, working)) = parse_status_line(trimmed) {
                cur.total_devices = total;
                cur.working_devices = working;
                cur.failed_devices = total.saturating_sub(working)
                    .max(count_failed_from_state(trimmed));
            }
        }
    }
    if let Some(e) = current { out.push(e); }
    out
}

/// Convert a MdEntry to the Value::Object the plugin publishes.
pub fn entry_to_value(e: &MdEntry) -> Value {
    let mut obj = BTreeMap::new();
    obj.insert("raid_name".into(), Value::String(e.name.clone()));
    obj.insert("status".into(), Value::String(e.status.clone()));
    obj.insert("level".into(), Value::String(e.level.clone()));
    obj.insert("total".into(), Value::Uint(e.total_devices as u64));
    obj.insert("working".into(), Value::Uint(e.working_devices as u64));
    obj.insert("failed".into(), Value::Uint(e.failed_devices as u64));
    obj.insert(
        "components".into(),
        Value::Array(
            e.components
                .iter()
                .map(|c| Value::String(c.clone()))
                .collect(),
        ),
    );
    Value::Object(obj)
}

pub struct RaidPlugin { base: GlancesPluginModel }

impl RaidPlugin {
    pub fn new() -> Self {
        Self { base: GlancesPluginModel::new(NAME, Value::Array(Vec::new())) }
    }
}

impl Plugin for RaidPlugin {
    fn name(&self) -> &'static str { NAME }
    fn reset(&mut self) { self.base.reset(); }
    fn stats(&self) -> &Value { &self.base.stats }
    fn model(&self) -> Option<&GlancesPluginModel> { Some(&self.base) }
    fn model_mut(&mut self) -> Option<&mut GlancesPluginModel> { Some(&mut self.base) }
    fn stats_mut(&mut self) -> &mut Value { &mut self.base.stats }
    fn get_key(&self) -> Option<&'static str> { Some("raid_name") }
    fn update(&mut self) -> Result<()> {
        // Per task: unreadable /proc/mdstat → empty array.
        let text = match fs::read_to_string("/proc/mdstat") {
            Ok(t) => t,
            Err(_) => {
                self.base.stats = Value::Array(Vec::new());
                return Ok(());
            }
        };
        let entries = parse_mdstat(&text);
        let arr = entries.iter().map(entry_to_value).collect();
        self.base.stats = Value::Array(arr);
        Ok(())
    }
}