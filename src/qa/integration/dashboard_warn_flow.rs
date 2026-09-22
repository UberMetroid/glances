//! Dashboard warning flow: click dims a warning until it clears,
//! double-click ignores it into a restorable footer line.

use super::dashboard_harness::assert_flow;

const WARN_CHECKS: &str = r##"  assert(document.title === "[!] glances-rs",
    "tab title must flag critical, got " + document.title);
  const wbox = byId.get("warnings");
  assert(wbox._k && wbox._k.size === 2, "expected 2 warning rows, got " + (wbox._k && wbox._k.size));
  const r0 = wbox._k.get("0:swap");
  assert(r0 && r0._l && typeof r0._l.click === "function", "rows must listen for clicks");
  r0._l.click();
  assert((lstore.get("glances_ack") || "").indexOf("swap") !== -1, "click must ack the warning");
  assert(r0.className.indexOf("acked") !== -1, "acked row must dim, got " + r0.className);
  r0._l.click();
  assert((lstore.get("glances_ack") || "[]") === "[]", "second click must un-ack");
  r0._l.dblclick();
  assert((lstore.get("glances_ignored") || "").indexOf("swap") !== -1, "double-click must ignore");
  assert(r0.style.display === "none", "ignored row must hide");
  const sp = wbox._foot._spans.filter((s) => s.textContent === "swap")[0];
  assert(sp && sp._l && typeof sp._l.click === "function", "footer names must restore on click");
  sp._l.click();
  assert((lstore.get("glances_ignored") || "[]") === "[]", "restore must clear ignore");
  assert(r0.style.display === "", "restored row must show");
"##;

#[test]
fn dashboard_warnings_ack_and_ignore() {
    assert_flow("warn", WARN_CHECKS);
}
