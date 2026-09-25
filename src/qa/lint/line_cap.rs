//! File-size lint: every .rs file is 16–256 lines (comments and blank
//! lines count). Only mod.rs/lib.rs module-wiring shims are exempt from
//! the floor — anything else under 16 lines folds into its closest
//! functionally-aligned sibling.

use std::fs;
use std::path::Path;

const FLOOR: usize = 16;
const CAP: usize = 256;

#[test]
fn every_rs_file_within_16_to_256_lines() {
    let src = Path::new("src");
    let mut violations = Vec::new();
    visit_rs(src, &mut |p, content| {
        let n = content.lines().count();
        let shim = p.file_name().and_then(|s| s.to_str()).is_some_and(|f| f == "mod.rs" || f == "lib.rs");
        if n > CAP {
            violations.push(format!("{}: {} lines (cap {})", p.display(), n, CAP));
        } else if n < FLOOR && !shim {
            violations.push(format!("{}: {} lines (floor {})", p.display(), n, FLOOR));
        }
    });
    assert!(violations.is_empty(),
        "file-size violation, want 16–256 lines:\n{}", violations.join("\n"));
}

fn visit_rs(dir: &Path, cb: &mut dyn FnMut(&Path, &str)) {
    if let Ok(rd) = fs::read_dir(dir) {
        for ent in rd.flatten() {
            let p = ent.path();
            if p.is_dir() { visit_rs(&p, cb); }
            else if p.extension().and_then(|s| s.to_str()) == Some("rs")
                && let Ok(t) = fs::read_to_string(&p) { cb(&p, &t); }
        }
    }
}
