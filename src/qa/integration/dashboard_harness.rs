//! Shared node harness for dashboard page-flow tests: loads the
//! SHIPPED dashboard JS (same `include_str!` bytes the server embeds)
//! with stubbed browser globals. Each flow file appends its own
//! checks to the prelude; the epilogue closes the driver.
//!
//! Needs `node` (preinstalled on CI runners and dev machines), the
//! same way the installer tests need `sh`.

use std::process::Command;

use crate::qa::harness::TempDir;

const DASHBOARD_HTML: &str = include_str!("../../../assets/static/templates/dashboard.html");

// Stubs + page load, up to the flow checks. Ends inside the async
// driver with `calls`, `store`, fakes, `assert`, `state` in scope.
const PRELUDE_JS: &str = r##""use strict";
// argv: node harness.js dashboard.html scenario; flow checks appended.
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
// Foldable sections: two fake sections with working toggles.
function mkSection(key) {
  const btn = { textContent: "", _click: null,
    addEventListener: function (ev, fn) { if (ev === "click") btn._click = fn; } };
  const h2 = { _dbl: null,
    addEventListener: function (ev, fn) { if (ev === "dblclick") h2._dbl = fn; } };
  const cls = { _s: {},
    add: function (c) { cls._s[c] = true; },
    remove: function (c) { delete cls._s[c]; },
    contains: function (c) { return !!cls._s[c]; } };
  return { _btn: btn, _cls: cls, _h2: h2,
    getAttribute: function (a) { return a === "data-sec" ? key : null; },
    querySelector: function (sel) { return sel === "h2" ? h2 : btn; },
    classList: cls };
}
const fakeSections = [mkSection("warnings"), mkSection("sensors")];
if (scenario === "fold") lstore.set("glances_folded", JSON.stringify({ sensors: true }));
// State pill: records its pause click. Page keystrokes route here.
let stateClick = null;
function mkState() {
  return { textContent: "", className: "", style: {},
    setAttribute: function () {},
    addEventListener: function (ev, fn) { if (ev === "click") stateClick = fn; } };
}
let keyHandler = null;
globalThis.document = {
  getElementById: (id) => {
    if (!byId.has(id)) byId.set(id, id === "theme" ? mkThemeButton()
      : id === "state" ? mkState() : mkEl());
    return byId.get(id);
  },
  querySelectorAll: (sel) => sel === "section[data-sec]" ? fakeSections : [],
  createElement: (t) => mkEl(),
  createElementNS: (ns, t) => mkEl(),
  addEventListener: function (ev, fn) { if (ev === "keydown") keyHandler = fn; },
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
  if (scenario === "sent" || scenario === "pause") {
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
"##;

// Closes the async driver: rejection check + PASS line.
const EPILOGUE_JS: &str = r##"
  assert(rejections.length === 0, "unhandled rejections: " + rejections.join(" | "));
  console.log("PASS[" + scenario + "]");
})();
"##;

pub(crate) fn run_flow(dir: &TempDir, scenario: &str, checks: &str) -> std::process::Output {
    let root = dir.path().to_path_buf();
    std::fs::write(root.join("dashboard.html"), DASHBOARD_HTML).unwrap();
    std::fs::write(root.join("harness.js"), format!("{PRELUDE_JS}{checks}{EPILOGUE_JS}")).unwrap();
    Command::new("node")
        .arg(root.join("harness.js"))
        .arg(root.join("dashboard.html"))
        .arg(scenario)
        .output()
        .expect("node is required for dashboard flow tests")
}

pub(crate) fn assert_flow(scenario: &str, checks: &str) {
    let dir = TempDir::new("dash-flow");
    let out = run_flow(&dir, scenario, checks);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "scenario {} failed:\nstdout: {}\nstderr: {}", scenario, stdout, stderr);
    assert!(stdout.contains("PASS"), "scenario {} missing PASS: {}", scenario, stdout);
}
