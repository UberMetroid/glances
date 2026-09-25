//! AC-2 lint: every .rs file is ≤ 256 lines (comments and blank lines count).

use std::fs;
use std::path::Path;

const CAP: usize = 256;

#[test]
fn every_rs_file_under_256_lines() {
    let src = Path::new("src");
    let mut violations = Vec::new();
    visit_rs(src, &mut |p, content| {
        let n = content.lines().count();
        if n > CAP {
            violations.push(format!("{}: {} lines (cap {})", p.display(), n, CAP));
        }
    });
    assert!(violations.is_empty(),
        "AC-2 violation: files exceed 256 lines:\n{}", violations.join("\n"));
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
