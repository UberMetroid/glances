//! Dashboard API-key flow: load the SHIPPED dashboard JS (same
//! `include_str!` bytes the server embeds) as a node module with
//! stubbed browser globals. Verifies: 401 prompts once then stays
//! quiet on cancel, accept stores + reloads, a stored key rides
//! every fetch. The `theme` scenario drives the split theme button
//! through a full cycle instead.
//!
//! Needs `node` (preinstalled on CI runners and dev machines), the
//! same way the installer tests need `sh`.

use std::process::Command;

use crate::qa::harness::TempDir;

const DASHBOARD_HTML: &str = include_str!("../../../assets/static/templates/dashboard.html");

const HARNESS_JS: &str = r##""use strict";
// argv: node harness.js dashboard.html scenario(cancel|accept|sent|theme).
// Stubs browser globals, loads the shipped script, drives one flow.
const fs = require("fs");
const path = require("path");
const html = fs.readFileSync(process.argv[2], "utf8");
const scenario = process.argv[3];
// Run the shipped page script through the module loader: same bytes
// the server embeds, loaded as a file (no dynamic code execution).
const pagePath = path.join(__dirname, "page.js");
fs.writeFileSync(pagePath, html.split("<script>")[1].split("</script>")[0]);

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
// Theme button: records its halves and click listener, links
// appended children like a real DOM (rebuild once, reuse after).
const themeKids = [];
let themeClick = null;
function mkThemeButton() {
  const btn = {
    firstElementChild: null, textContent: "", className: "", style: {},
    append: function (a, b) {
      themeKids.push(a, b);
      a.nextElementSibling = b;
      btn.firstElementChild = a;
    },
    setAttribute: function () {},
    getAttribute: function () { return null; },
    addEventListener: function (ev, fn) { if (ev === "click") themeClick = fn; },
    hasChildNodes: function () { return true; },
  };
  return btn;
}
const lstore = new Map();
globalThis.localStorage = {
  getItem: (k) => (lstore.has(k) ? lstore.get(k) : null),
  setItem: (k, v) => { lstore.set(k, String(v)); },
  removeItem: (k) => { lstore.delete(k); },
};
globalThis.document = {
  getElementById: (id) => {
    if (!byId.has(id)) byId.set(id, id === "theme" ? mkThemeButton() : mkEl());
    return byId.get(id);
  },
  createElement: (t) => mkEl(),
  createElementNS: (ns, t) => mkEl(),
  querySelectorAll: (sel) => [],
  addEventListener: function () {},
};

const VALUES = {
  cpu: { total: 12.5, user: 5.0, system: 4.0, iowait: 1.0, steal: 0.0 },
  percpu: [{ cpu_number: 0, total: 33.3 }],
  sensors: [{ kind: "temperature_c", label: "cpu", value: 55.5 }],
  gpu: [{ kind: "internal", vendor: "Intel", name: "iGPU", util_pct: 10,
    freq_mhz: 1200, pci: "00:02.0", mem_used_mb: 100, mem_total_mb: 1000, temp_c: 50,
    clients: [{ pid: 4321, name: "ffmpeg", service: "jellyfin", mem_mb: null, transcoding: true },
              { pid: 1209, name: "python", service: "invokeai", mem_mb: 4548, transcoding: false }],
    transcoding: true, transcoding_by: "jellyfin" }],
  network: [{ interface_name: "eth0", is_up: true,
    bytes_recv_rate_per_sec: 1024, bytes_sent_rate_per_sec: 2048 }],
  diskio: [{ disk_name: "sda", read_bytes_rate_per_sec: 4096, write_bytes_rate_per_sec: 8192 }],
  fs: [{ mnt_point: "/", percent: 42.5 }],
  alert: [{ type: "cpu", stat: "total", value: 95.5 }],
  processlist: [
    { pid: 1, name: "init", cpu_percent: 0.1, memory_percent: 0.2,
      memory_info: { rss: 1024 }, num_threads: 1, status: "sleeping" },
    { pid: 2, name: "kthreadd", cpu_percent: 0.0, memory_percent: 0.0,
      memory_info: { rss: 0 }, num_threads: 1, status: "sleeping" },
  ],
};
// 40 processes: the table must render all of them, never truncate.
for (let i = 3; i <= 40; i++) {
  VALUES.processlist.push({ pid: i, name: "proc" + i, cpu_percent: 0.1,
    memory_percent: 0.1, memory_info: { rss: 1024 }, num_threads: 1, status: "sleeping" });
}
function resp(ok, status, body) {
  return { ok: ok, status: status, json: async () => body };
}
globalThis.fetch = async (url, opts) => {
  calls.fetch.push({ url: url, headers: (opts && opts.headers) || {} });
  if (scenario === "sent") {
    if (url === "api/4/dashboard")
      return resp(true, 200, Object.assign({ health: { status: "ok", summary: "ALL SYSTEMS NOMINAL", checks: [] } }, VALUES));
    const m = typeof url === "string" && url.match(/^api\/4\/([a-z]+)$/);
    if (m && VALUES[m[1]] !== undefined) return resp(true, 200, VALUES[m[1]]);
    return resp(true, 200, {});
  }
  return resp(false, 401, null);
};

let tickFns = [];
globalThis.setInterval = (fn, ms) => { tickFns.push({ fn: fn, ms: ms }); return tickFns.length; };
const rejections = [];
process.on("unhandledRejection", (e) => { rejections.push(String(e)); });

function assert(c, msg) {
  if (!c) { console.error("FAIL[" + scenario + "]: " + msg); process.exit(1); }
}
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  require("./page.js");
  await sleep(150);
  const state = () => byId.get("state").textContent;
  if (scenario === "cancel") {
    assert(calls.prompts.length === 1, "expected 1 prompt, got " + calls.prompts.length);
    assert(/API key/.test(calls.prompts[0]), "prompt must ask for the API key");
    assert(!store.has("glances_key"), "cancel must not store a key");
    assert(calls.reloads === 0, "cancel must not reload");
    assert(state() === "unauthorized \u2014 reload to retry", "bad cancel note: " + state());
    for (const t of tickFns) { await t.fn(); }
    await sleep(50);
    assert(calls.prompts.length === 1, "second 401 must stay quiet");
  } else if (scenario === "accept") {
    assert(calls.prompts.length === 1, "expected 1 prompt");
    assert(store.get("glances_key") === "k3y", "key must be stored");
    assert(calls.reloads === 1, "accept must reload once");
  } else if (scenario === "sent") {
    assert(calls.prompts.length === 0, "stored key must not prompt");
    assert(calls.reloads === 0, "stored key must not reload");
    assert(tickFns.length === 2, "expected fast+slow polls, got " + tickFns.length);
    assert(tickFns[0].ms === 2000 && tickFns[1].ms === 10000,
      "bad poll cadence: " + tickFns.map((t) => t.ms).join(","));
    const urls = calls.fetch.map((c) => c.url);
    assert(urls.indexOf("api/all/values") === -1, "bulk poll must be gone");
    ["api/4/dashboard", "api/4/processlist", "api/4/alert", "api/4/cpu/history/60",
     "api/4/mem/history/60"].forEach((u) => {
      assert(urls.indexOf(u) !== -1, "missing fetch: " + u);
    });
    ["api/4/cpu", "api/4/gpu", "api/4/sensors", "api/4/health"].forEach((u) => {
      assert(urls.indexOf(u) === -1, "per-plugin fast poll must be gone: " + u);
    });
    calls.fetch.forEach((c) => {
      assert(c.headers["X-API-Key"] === "k3y", "missing key header on " + c.url);
    });
    assert(state().indexOf("live ") === 0, "expected live state, got: " + state());
    const plistKeys = byId.get("plist")._k;
    assert(plistKeys && plistKeys.size === 40,
      "expected 40 keyed process rows, got " + (plistKeys && plistKeys.size));
  } else if (scenario === "theme") {
    assert(themeClick !== null, "theme button must register a click listener");
    assert(themeKids.length === 2,
      "theme button must build two halves, got " + themeKids.length);
    const expect = (cur, next) => {
      assert(themeKids[0].textContent === cur + "→", "left half: " + themeKids[0].textContent);
      assert(themeKids[0].className === "tn-" + cur, "left class: " + themeKids[0].className);
      assert(themeKids[1].textContent === next, "right half: " + themeKids[1].textContent);
      assert(themeKids[1].className === "tn-" + next, "right class: " + themeKids[1].className);
      assert(lstore.get("glances_theme") === cur, "stored theme: " + lstore.get("glances_theme"));
    };
    expect("1982", "1992");
    themeClick(); expect("1992", "2002");
    themeClick(); expect("2002", "2022");
    themeClick(); expect("2022", "1982");
    themeClick(); expect("1982", "1992");
    assert(themeKids.length === 2,
      "repaints must reuse the halves, got " + themeKids.length);
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

#[test]
fn dashboard_theme_button_cycles_current_next() {
    assert_flow("theme");
}
