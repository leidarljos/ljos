//! Regression tests for terminal prose wrapping edge cases.

ljos_test_boot::harness!();

use ljos_cli::output_wrap::{wrap_hanging_prose, wrap_prose};
use ljos_cli::scan;

#[test]
fn non_tty_scan_wrap_matches_legacy_break_word_behavior() {
    let solid =
        "SupercalifragilisticexpialidociousKilnDamperReadingThatExceedsEightyColumnsWithoutABreak";
    let wrapped = scan::wrap_for(solid, scan::Width::FIXED);
    assert!(wrapped.contains('\\'), "{wrapped}");
    assert!(
        wrapped
            .lines()
            .all(|line| line.chars().count() <= scan::COLS),
        "{wrapped}"
    );
}

#[test]
fn multibyte_text_inside_backticks_does_not_panic() {
    let input = "note `café 你好` keeps moving with more words";
    let wrapped = wrap_prose(input, Some(20));
    assert!(wrapped.contains("`café 你好`"), "{wrapped}");
}

#[test]
fn too_wide_hanging_prefix_falls_back_without_overflow() {
    let first = "demo-uqy6  STARTED  [#C]  ";
    let cont = "                        ";
    let wrapped = wrap_hanging_prose("short body", 20, first, cont);
    assert!(
        wrapped.lines().next().is_none_or(|line| line != first),
        "{wrapped}"
    );
    for line in wrapped.lines() {
        let wide = line.chars().count() > 20;
        let lone = line.split_whitespace().count() <= 1;
        assert!(!wide || lone, "{wrapped}");
    }
}
