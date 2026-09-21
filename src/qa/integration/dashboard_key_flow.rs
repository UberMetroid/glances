//! Dashboard API-key flow: drive the SHIPPED dashboard JS (same
//! `include_str!` bytes the server embeds) under node with stubbed
//! browser globals. Verifies: 401 prompts once then stays quiet on
//! cancel, accept stores + reloads, a stored key rides every fetch.
//!
//! Needs `node` (preinstalled on CI runners and dev machines), the
//! same way the installer tests need `sh`.

use std::process::Command;

use crate::qa::harness::TempDir;

const DASHBOARD_HTML: &str = include_str!("../../../assets/static/templates/dashboard.html");

const HARNESS_JS: &str = r##""use strict";
// argv: node harness.js dashboard.html scenario(cancel|accept|sent).
// Stubs browser globals, evals the shipped script, drives one flow.
const fs = require("fs");
const html = fs.readFileSync(process.argv[2], "utf8");
const scenario = process.argv[3];
const script = html.split("<script>")[1].split("</script>")[0];

const calls = { fetch: [], prompts: [], reloads: 0 };
const store = new Map();
if (scenario === "sent") store.set("glances_key", "k3y");

globalThis.sessionStorage = {
  getItem: (k) => (store.has(k) ? store.get(k) : null),
  setItem: (k, v) => { store.set(k, String(v)); },
  removeItem: (k) => { store.delete(k); },
};
globalThis.prompt = (msg) => {
  calls.prompts.push(String(msg));
  return scenario === "accept" ? "k3y" : null;
};
globalThis.location = { reload: () => { calls.reloads++; } };

function mkEl() {
  return {
    textContent: "", className: "", style: {},
    firstElementChild: { style: {} },
    append: function () {},
    setAttribute: function () {},
    getAttribute: function () { return null; },
    addEventListener: function () {},
    hasChildNodes: function () { return true; },
  };
}
const byId = new Map();
globalThis.document = {
  getElementById: (id) => {
    if (!byId.has(id)) byId.set(id, mkEl());
    return byId.get(id);
  },
  createElement: (t) => mkEl(),
  createElementNS: (ns, t) => mkEl(),
  querySelectorAll: (sel) => [],
};

const VALUES = {
  cpu: { total: 12.5, user: 5.0, system: 4.0, iowait: 1.0, steal: 0.0 },
  processlist: [
    { pid: 1, name: "init", cpu_percent: 0.1, memory_percent: 0.2,
      memory_info: { rss: 1024 }, num_threads: 1, status: "sleeping" },
    { pid: 2, name: "kthreadd", cpu_percent: 0.0, memory_percent: 0.0,
      memory_info: { rss: 0 }, num_threads: 1, status: "sleeping" },
  ],
};
function resp(ok, status, body) {
  return { ok: ok, status: status, json: async () => body };
}
globalThis.fetch = async (url, opts) => {
  calls.fetch.push({ url: url, headers: (opts && opts.headers) || {} });
  if (scenario === "sent") {
    if (url === "api/all/values") return resp(true, 200, VALUES);
    if (url === "api/4/health") return resp(true, 200, { checks: [] });
    return resp(true, 200, {});
  }
  return resp(false, 401, null);
};

let tickFn = null;
globalThis.setInterval = (fn, ms) => { tickFn = fn; return 1; };
const rejections = [];
process.on("unhandledRejection", (e) => { rejections.push(String(e)); });

function assert(c, msg) {
  if (!c) { console.error("FAIL[" + scenario + "]: " + msg); process.exit(1); }
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  eval(script);
  await sleep(150);
  const state = () => byId.get("state").textContent;
  if (scenario === "cancel") {
    assert(calls.prompts.length === 1, "expected 1 prompt, got " + calls.prompts.length);
    assert(/API key/.test(calls.prompts[0]), "prompt must ask for the API key");
    assert(!store.has("glances_key"), "cancel must not store a key");
    assert(calls.reloads === 0, "cancel must not reload");
    assert(state() === "unauthorized \u2014 reload to retry", "bad cancel note: " + state());
    await tickFn();
    await sleep(50);
    assert(calls.prompts.length === 1, "second 401 must stay quiet");
  } else if (scenario === "accept") {
    assert(calls.prompts.length === 1, "expected 1 prompt");
    assert(store.get("glances_key") === "k3y", "key must be stored");
    assert(calls.reloads === 1, "accept must reload once");
  } else if (scenario === "sent") {
    assert(calls.prompts.length === 0, "stored key must not prompt");
    assert(calls.reloads === 0, "stored key must not reload");
    const urls = calls.fetch.map((c) => c.url);
    ["api/all/values", "api/4/health", "api/4/cpu/history/60",
     "api/4/mem/history/60", "api/4/cpu"].forEach((u) => {
      assert(urls.indexOf(u) !== -1, "missing fetch: " + u);
    });
    calls.fetch.forEach((c) => {
      assert(c.headers["X-API-Key"] === "k3y", "missing key header on " + c.url);
    });
    assert(state().indexOf("live ") === 0, "expected live state, got: " + state());
  } else {
    assert(false, "unknown scenario");
  }
  assert(rejections.length === 0, "unhandled rejections: " + rejections.join(" | "));
  console.log("PASS[" + scenario + "]");
})();
"##;

fn run_flow(dir: &TempDir, scenario: &str) -> std::process::Output {
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("dashboard.html"), DASHBOARD_HTML).unwrap();
    std::fs::write(root.join("harness.js"), HARNESS_JS).unwrap();
    Command::new("node")
        .arg(root.join("harness.js"))
        .arg(root.join("dashboard.html"))
        .arg(scenario)
        .output()
        .expect("node is required for dashboard flow tests")
}

fn assert_flow(scenario: &str) {
    let dir = TempDir::new("dash-key-flow");
    let out = run_flow(&dir, scenario);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "scenario {} failed:\nstdout: {}\nstderr: {}", scenario, stdout, stderr);
    assert!(stdout.contains("PASS"), "scenario {} missing PASS: {}", scenario, stdout);
}

#[test]
fn dashboard_key_cancel_prompts_once_then_quiet() {
    assert_flow("cancel");
}

#[test]
fn dashboard_key_accept_stores_and_reloads() {
    assert_flow("accept");
}

#[test]
fn dashboard_key_sent_on_every_fetch() {
    assert_flow("sent");
}
