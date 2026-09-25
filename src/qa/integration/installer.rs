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
        if let Ok(out) = Command::new(bin).args(args).arg(path).output()
            && out.status.success() {
                return String::from_utf8_lossy(&out.stdout)
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_string();
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
fn verify_script_passes_shell_syntax_check() {
    let out = Command::new("sh")
        .arg("-n")
        .arg(root().join("verify-deploy.sh"))
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

#[test]
fn installer_key_step_round_trips() {
    use crate::qa::harness::TempDir;
    use std::os::unix::fs::PermissionsExt;
    // Execute the real Step 5 block (from "# Step 5" to EOF) with
    // stubbed log helpers and a fake HOME. Child-only env (no process
    // mutation): HOME=fake, XDG_CONFIG_HOME removed, key per case.
    let script = read("install.sh");
    let step = script.split("# Step 5").nth(1).expect("Step 5 block");
    let stubs = "info() { echo \"I:$1\"; }; success() { echo \"S:$1\"; }; warn() { echo \"W:$1\" >&2; }; err() { echo \"E:$1\" >&2; exit 1; }\n";
    let run = |home: &std::path::Path, key: Option<&str>| {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(format!("{}\n# Step 5{}", stubs, step));
        cmd.env("HOME", home).env_remove("XDG_CONFIG_HOME");
        match key {
            Some(k) => { cmd.env("GLANCES_API_KEY", k); }
            None => { cmd.env_remove("GLANCES_API_KEY"); }
        }
        cmd.output().expect("sh should run")
    };
    let key_file = |home: &std::path::Path| home.join(".config/glances/api-key");
    // Fresh HOME + key: exact bytes, mode 0600.
    let d1 = TempDir::new("key-step-fresh");
    let out = run(d1.path(), Some("k3y"));
    assert!(out.status.success(), "fresh+key must succeed");
    let p1 = key_file(d1.path());
    assert_eq!(std::fs::read_to_string(&p1).unwrap(), "k3y");
    assert_eq!(std::fs::metadata(&p1).unwrap().permissions().mode() & 0o777, 0o600);
    // Fresh HOME + blank: nothing written, stays open.
    let d2 = TempDir::new("key-step-blank");
    let out = run(d2.path(), None);
    assert!(out.status.success(), "fresh+blank must succeed");
    assert!(!key_file(d2.path()).exists(), "blank must not create a key");
    assert!(String::from_utf8_lossy(&out.stdout).contains("will be open"));
    // Existing file + blank: kept untouched.
    let d3 = TempDir::new("key-step-keep");
    let h3 = d3.path().to_path_buf();
    let p3 = key_file(&h3);
    std::fs::create_dir_all(p3.parent().unwrap()).unwrap();
    std::fs::write(&p3, "old").unwrap();
    let out = run(&h3, None);
    assert!(out.status.success(), "keep must succeed");
    assert_eq!(std::fs::read_to_string(&p3).unwrap(), "old");
    assert!(String::from_utf8_lossy(&out.stderr).contains("kept"));
    // Newline or whitespace-only key: loud failure, nothing written.
    let d4 = TempDir::new("key-step-bad");
    let h4 = d4.path().to_path_buf();
    assert!(!run(&h4, Some("a\nb")).status.success(), "newline key must fail");
    assert!(!run(&h4, Some("   ")).status.success(), "blank key must fail");
    assert!(!key_file(&h4).exists(), "failed key must not write");
}
