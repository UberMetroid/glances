//! Ownership-ledger accuracy: every .rs file under src/ appears exactly
//! once in docs/ownership.md with a valid status. This checks the ledger
//! is accurate, not that the rewrite is done — completion (65/65 OWNED)
//! is read by a human so this lint never goes red mid-rewrite.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

#[test]
fn ledger_covers_every_rs_file_exactly_once() {
    let ledger = fs::read_to_string("docs/ownership.md").expect("docs/ownership.md must exist");
    let mut entries: BTreeMap<String, String> = BTreeMap::new();
    let mut problems = Vec::new();
    for line in ledger.lines() {
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() < 4 || !cells[1].ends_with(".rs") { continue; }
        if cells[2] != "TAINTED" && cells[2] != "OWNED" {
            problems.push(format!("bad status for {}: {:?}", cells[1], cells[2]));
            continue;
        }
        if entries.insert(cells[1].to_string(), cells[2].to_string()).is_some() {
            problems.push(format!("duplicate entry: {}", cells[1]));
        }
    }
    let mut disk = BTreeSet::new();
    visit_rs(Path::new("src"), &mut |p| {
        disk.insert(p.to_string_lossy().replace('\\', "/"));
    });
    for f in &disk {
        if !entries.contains_key(f) {
            problems.push(format!("missing from ledger: {f}"));
        }
    }
    for f in entries.keys() {
        if !disk.contains(f) {
            problems.push(format!("ledger lists ghost file: {f}"));
        }
    }
    assert!(problems.is_empty(),
        "ownership ledger drift:\n{}", problems.join("\n"));
}

fn visit_rs(dir: &Path, cb: &mut dyn FnMut(&Path)) {
    if let Ok(rd) = fs::read_dir(dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() { visit_rs(&p, cb); }
            else if p.extension().and_then(|s| s.to_str()) == Some("rs") { cb(&p); }
        }
    }
}
