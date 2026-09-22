//! Dashboard pause flow: Space (or clicking the state pill)
//! freezes all polling; pressing again resumes where it left off.

use super::dashboard_harness::assert_flow;

const PAUSE_CHECKS: &str = r##"  assert(typeof keyHandler === "function", "page must listen for keys");
  assert(stateClick !== null, "state pill must toggle pause on click");
  const n0 = calls.fetch.length;
  assert(n0 > 0, "expected initial fetches, got " + n0);
  keyHandler({ key: " ", preventDefault: function () {}, target: { tagName: "BODY" } });
  assert(byId.get("state").textContent === "paused",
    "state must show paused, got " + byId.get("state").textContent);
  await tickFns[0].fn();
  await tickFns[1].fn();
  assert(calls.fetch.length === n0, "paused ticks must not fetch");
  stateClick();
  await tickFns[0].fn();
  assert(calls.fetch.length > n0, "resumed ticks must fetch, got " + calls.fetch.length);
"##;

#[test]
fn dashboard_space_pauses_and_pill_resumes_polls() {
    assert_flow("pause", PAUSE_CHECKS);
}
