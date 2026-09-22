//! Dashboard API-key flow: 401 prompts once then stays quiet on
//! cancel, accept stores + reloads, a stored key rides every fetch.

use super::dashboard_harness::assert_flow;

const KEY_CHECKS: &str = r##"  if (scenario === "cancel") {
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
    assert(document.title === "● glances-rs",
      "tab title must show health, got " + document.title);
    assert(byId.get("net-total-h").textContent === "3.0K/s",
      "network header must total rates, got " + byId.get("net-total-h").textContent);
    assert(byId.get("procfilter").textContent === "· 40",
      "process header must count rows, got " + byId.get("procfilter").textContent);
    assert(byId.get("plist")._k.get("p1")._c[4].textContent === "1.5M/s",
      "process rows must total disk rates");
    const strainKeys = byId.get("strain")._k;
    assert(strainKeys && strainKeys.size === 3, "strain must render three rows");
    assert(strainKeys.get("stCPU")._v.textContent === "12.5%",
      "strain must show pressure values");
    await tickFns[1].fn();
    await tickFns[0].fn();
    ["age-processlist", "age-alert"].forEach((id) => {
      const a = byId.get(id);
      assert(/^\d+s$/.test(a.textContent), id + " must show its age, got " + a.textContent);
      assert(a.className === "age", id + " must not flag fresh data, got " + a.className);
    });
    await tickFns[1].fn();
    assert(byId.get("host").textContent === "testbox",
      "slow tick must not blank the header, got " + byId.get("host").textContent);
    assert(byId.get("uptime").textContent === "01:01",
      "slow tick must not blank uptime, got " + byId.get("uptime").textContent);
    assert(byId.get("load").textContent === "1.50 1.00 0.50",
      "slow tick must not blank load, got " + byId.get("load").textContent);
    assert(byId.get("cpu-total-h").textContent === "12.5%",
      "slow tick must not blank cpu, got " + byId.get("cpu-total-h").textContent);
    assert(byId.get("cpu-bar").firstElementChild.style.width === "12.5%",
      "slow tick must not reset the cpu bar");
  } else {
    assert(false, "unknown scenario");
  }
"##;

#[test]
fn dashboard_key_cancel_prompts_once_then_quiet() {
    assert_flow("cancel", KEY_CHECKS);
}

#[test]
fn dashboard_key_accept_stores_and_reloads() {
    assert_flow("accept", KEY_CHECKS);
}

#[test]
fn dashboard_key_sent_on_every_fetch() {
    assert_flow("sent", KEY_CHECKS);
}
