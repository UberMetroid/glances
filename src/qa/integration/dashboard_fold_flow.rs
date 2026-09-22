//! Dashboard fold flow: sections restore folded from storage,
//! toggles flip the folded class, clicks persist the map,
//! double-clicking a header folds or unfolds everything.

use super::dashboard_harness::assert_flow;

const FOLD_CHECKS: &str = r##"  const w = fakeSections[0], s = fakeSections[1];
  assert(w._btn.textContent === "–", "warnings must start open, got " + w._btn.textContent);
  assert(!w._cls.contains("folded"), "warnings must start unfolded");
  assert(s._btn.textContent === "+", "sensors must restore folded, got " + s._btn.textContent);
  assert(s._cls.contains("folded"), "sensors must restore the folded class");
  assert(w._btn._click !== null && s._btn._click !== null, "fold toggles must listen for clicks");
  s._btn._click();
  assert(s._btn.textContent === "–", "sensors must unfold on click");
  assert(!s._cls.contains("folded"), "sensors must drop the folded class");
  w._btn._click();
  assert(w._btn.textContent === "+", "warnings must fold on click");
  assert(w._cls.contains("folded"), "warnings must gain the folded class");
  const saved = JSON.parse(lstore.get("glances_folded") || "{}");
  assert(saved.warnings === true && saved.sensors === false,
    "fold state must persist, got " + lstore.get("glances_folded"));
  const h2w = fakeSections[0]._h2, h2s = fakeSections[1]._h2;
  assert(h2w._dbl !== null, "headers must listen for double-click");
  h2w._dbl({ target: { className: "" } });
  assert(w._cls.contains("folded") && s._cls.contains("folded"),
    "double-click must fold all open sections");
  h2s._dbl({ target: { className: "" } });
  assert(!w._cls.contains("folded") && !s._cls.contains("folded"),
    "double-click must unfold everything");
  h2w._dbl({ target: { className: "fold" } });
  assert(!w._cls.contains("folded") && !s._cls.contains("folded"),
    "double-click on the button must not fold all");
"##;

#[test]
fn dashboard_sections_fold_and_persist() {
    assert_flow("fold", FOLD_CHECKS);
}
