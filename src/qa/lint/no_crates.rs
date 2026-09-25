//! AC-1 lint: no `[dependencies]` in Cargo.toml, no `extern crate` in src/.

use std::fs;
use std::path::Path;

const CARGO_TOML: &str = "Cargo.toml";

#[test]
fn cargo_toml_has_no_dependencies() {
    let txt = fs::read_to_string(CARGO_TOML).expect("Cargo.toml present");
    // We allow [package], [lib], [[bin]], [profile.*], [features].
    // Anything else is forbidden.
    let mut in_allowed = false;
    let mut current_section = String::new();
    let mut violations = Vec::new();
    for (lineno, raw) in txt.lines().enumerate() {
        let line = raw.trim();
        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len()-1].to_string();
            in_allowed = matches!(current_section.as_str(),
                "package" | "lib" | "bin" | "features"
            ) || current_section.starts_with("profile.");
            // Also allow our own [[bin]] section header.
            if line.starts_with("[[") { in_allowed = true; }
            continue;
        }
        if !in_allowed && !line.is_empty() && !line.starts_with('#') {
            violations.push(format!("line {}: unexpected content in [{}]: {}", lineno + 1, current_section, line));
        }
    }
    assert!(violations.is_empty(),
        "AC-1 violation: Cargo.toml has unauthorized sections:\n{}",
        violations.join("\n"));
}

#[test]
fn no_extern_crate_in_src() {
    let src = Path::new("src");
    let mut bad = Vec::new();
    visit_rs(src, &mut |p, content| {
        let path_str = p.to_string_lossy().replace('\\', "/");
        if path_str.contains("/qa/lint/") { return; }
        if content.contains("extern crate ") {
            bad.push(path_str);
        }
    });
    assert!(bad.is_empty(),
        "AC-1 violation: `extern crate` found in: {:?}", bad);
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
