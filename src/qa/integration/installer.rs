//! Installer collateral: `install.sh` stays syntactically valid, the
//! published `.sha256` matches its bytes, no stale variable
//! references survive, and the script is linked from both the README
//! and the website.

use std::path::PathBuf;
use std::process::Command;

fn root() -> PathBuf {
    PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_string()))
}

fn read(name: &str) -> String {
    std::fs::read_to_string(root().join(name)).expect("repo file should exist")
}

fn sha256_of(path: &std::path::Path) -> String {
    // Prefer sha256sum, fall back to shasum -a 256.
    for (bin, args) in [("sha256sum", &[][..]), ("shasum", &["-a", "256"][..])] {
        if let Ok(out) = Command::new(bin).args(args).arg(path).output() {
            if out.status.success() {
                return String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
            }
        }
    }
    panic!("no sha256 tool found");
}

#[test]
fn installer_passes_shell_syntax_check() {
    let out = Command::new("sh")
        .arg("-n")
        .arg(root().join("install.sh"))
        .output()
        .expect("sh should run");
    assert!(out.status.success(), "sh -n failed: {:?}", out.status.code());
}

#[test]
fn installer_sha_matches_bytes() {
    let digest = sha256_of(&root().join("install.sh"));
    let published = read("install.sh.sha256");
    let published_hash = published.split_whitespace().next().unwrap_or("");
    assert_eq!(published_hash, digest, "install.sh.sha256 is stale; refresh it");
}

#[test]
fn installer_has_no_stale_references() {
    let script = read("install.sh");
    assert!(!script.contains("os_part"), "unset $os_part guard is back");
    assert!(!script.contains("/home/jeryd"), "developer-local path leaked");
}

#[test]
fn installer_linked_from_readme_and_site() {
    let url = "raw.githubusercontent.com/UberMetroid/glances-rs/rust/install.sh";
    assert!(read("README.md").contains(url), "README must link install.sh");
    let site = read("site/index.html");
    assert!(site.contains(url), "website must link install.sh");
    assert!(site.contains("install.sh.sha256"), "website must mention verification");
}
