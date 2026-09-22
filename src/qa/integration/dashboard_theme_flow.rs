//! Dashboard theme flow: the split button names the current theme
//! in its own colors and the next theme in its, through a cycle.

use super::dashboard_harness::assert_flow;

const THEME_CHECKS: &str = r##"  assert(themeClick !== null, "theme button must register a click listener");
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
"##;

#[test]
fn dashboard_theme_button_cycles_current_next() {
    assert_flow("theme", THEME_CHECKS);
}
