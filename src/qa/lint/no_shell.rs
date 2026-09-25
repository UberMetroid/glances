//! AC-1 / §3.5.4 lint: no shell expansion. `safe_run.rs` uses argv-only.

use std::fs;
use std::path::Path;

#[test]
fn no_sh_invocation_in_src() {
    let src = Path::new("src");
    let mut bad = Vec::new();
    visit_rs(src, &mut |p, content| {
        let path_str = p.to_string_lossy().replace('\\', "/");
        if path_str.contains("/qa/") { return; }
        let s = content;
        if s.contains("\"sh\"") || s.contains("\"bash\"") || s.contains("\"/bin/sh\"") || s.contains("\"/bin/bash\"") {
            bad.push(path_str);
        }
        if s.contains("cmd.exe /C") || s.contains("cmd.exe /c") {
            bad.push(p.display().to_string());
        }
    });
    assert!(bad.is_empty(),
        "AC violation: shell invocation found in: {:?}", bad);
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
