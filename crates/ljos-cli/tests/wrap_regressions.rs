//! Regression tests for terminal prose wrapping edge cases.

ljos_test_boot::harness!();

use ljos_cli::output_wrap::{wrap_hanging_prose, wrap_prose};
use ljos_cli::scan;

#[test]
fn multiline_hanging_keeps_prefix_blank_lines_and_inner_indent() {
    let body = "first line\n  Þetta er íslenskt: ofninn í Reykjavík þarf nýja þéttingu.\n\nsecond";
    let wrapped = wrap_hanging_prose(body, 28, "| ", "| ");
    let lines: Vec<&str> = wrapped.lines().collect();
    assert_eq!(lines.first().copied(), Some("| first line"));
    assert!(lines.contains(&"|"), "{wrapped}");
    assert!(
        lines
            .iter()
            .any(|line| line.starts_with("|   Þetta er íslenskt")),
        "{wrapped}"
    );
    assert!(lines.iter().all(|line| !line.ends_with(' ')), "{wrapped}");
}

#[test]
fn tabs_measure_to_the_next_eight_column_stop() {
    fn tabstop_width(text: &str) -> usize {
        let mut width = 0usize;
        for ch in text.chars() {
            if ch == '\x1b' {
                continue;
            }
            if ch == '\t' {
                width += 8 - (width % 8);
            } else {
                width += unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            }
        }
        width
    }

    let wrapped = wrap_prose("alpha\tbeta\tgamma\tdelta\tepsilon", Some(20));
    for line in wrapped.lines() {
        assert!(tabstop_width(line) <= 20, "{wrapped}");
    }
}

#[test]
fn non_tty_scan_wrap_matches_legacy_break_word_behavior() {
    let solid =
        "SupercalifragilisticexpialidociousKilnDamperReadingThatExceedsEightyColumnsWithoutABreak";
    let wrapped = scan::wrap(solid, 20);
    assert!(wrapped.contains('\\'), "{wrapped}");
    assert!(
        wrapped.lines().all(|line| line.chars().count() <= 20),
        "{wrapped}"
    );
}

#[test]
fn osc_with_spaces_stays_atomic_when_wrapped() {
    let wrapped = wrap_prose("pre \u{1b}]0;my title\u{7} post words", Some(10));
    assert!(wrapped.contains("\u{1b}]0;my title\u{7}"), "{wrapped}");
    assert!(!wrapped.contains("\u{1b}]0;\n"), "{wrapped}");
    assert!(!wrapped.contains("my\n title"), "{wrapped}");
}

#[test]
fn unterminated_escape_stays_atomic_for_width_and_segments() {
    let wrapped = wrap_prose(
        "pre \u{1b}]0;my title with spaces still one token",
        Some(12),
    );
    assert!(wrapped.contains("\u{1b}]0;my title with spaces still one token"));
    assert!(!wrapped.contains("\u{1b}]0;my\n"), "{wrapped}");
    assert!(!wrapped.contains("\u{1b}]0;my title\n"), "{wrapped}");
}

#[test]
fn unterminated_csi_stays_atomic_for_width_and_segments() {
    let wrapped = wrap_prose("pre \u{1b}[1;31 123 456 789", Some(10));
    assert!(!wrapped.contains("\u{1b}[1;31\n"), "{wrapped}");
    assert!(!wrapped.contains("31 123\n"), "{wrapped}");
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
