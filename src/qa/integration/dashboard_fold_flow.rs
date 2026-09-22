//! Dashboard fold flow: sections restore folded from storage,
//! toggles flip the folded class, clicks persist the map.

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
"##;

#[test]
fn dashboard_sections_fold_and_persist() {
    assert_flow("fold", FOLD_CHECKS);
}
