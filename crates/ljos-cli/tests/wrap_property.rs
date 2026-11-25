//! Seeded property checks for prose wrapping invariants.

ljos_test_boot::harness!();

use ljos_cli::output_wrap::{prose_display_width, strip_ansi, wrap_hanging_prose, wrap_prose};
use unicode_segmentation::UnicodeSegmentation;

/// The visible text of the bank tokens that a wrap may not split.
const UNBREAKABLE: [&str; 4] = [
    "`code span`",
    "``double ticks``",
    "https://example.com/a/very/long/path",
    "click here",
];

const ESCAPES: [&str; 6] = [
    "\u{1b}[31m",
    "\u{1b}[32m",
    "\u{1b}[1m",
    "\u{1b}[0m",
    "\u{1b}]0;my title\u{7}",
    "\u{1b}]8;;\u{7}",
];

fn strip_ws(s: &str) -> String {
    s.chars().filter(|ch| !ch.is_whitespace()).collect()
}

fn xorshift64(mut x: u64) -> u64 {
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    x
}

/// A line may pass `cols` only when it holds one unbreakable token or one
/// grapheme.
fn lone_overwide(line: &str, cols: usize) -> bool {
    let visible = strip_ansi(line);
    let visible = visible.trim();
    prose_display_width(line) > cols
        && (UNBREAKABLE.contains(&visible) || visible.graphemes(true).count() == 1)
}

fn assert_invariants(expected_input: &str, wrapped: &str, cols: usize) {
    for line in wrapped.lines() {
        assert!(!line.ends_with(' '), "trailing spaces: {line:?}");
        let width = prose_display_width(line);
        if width > cols {
            assert!(
                lone_overwide(line, cols),
                "line wider than cols={cols}: {line:?}"
            );
        }
    }
    assert_eq!(
        strip_ws(wrapped),
        strip_ws(expected_input),
        "whitespace-stripped text changed at cols={cols}"
    );
    assert!(!wrapped.contains("\u{1b}[\n"), "split CSI");
    assert!(!wrapped.contains("\u{1b}]\n"), "split OSC");
    for escape in ESCAPES {
        assert_eq!(
            wrapped.matches(escape).count(),
            expected_input.matches(escape).count(),
            "split {escape:?} at cols={cols}: {wrapped:?}"
        );
    }
    let chars: Vec<char> = wrapped.chars().collect();
    let mut i = 0usize;
    let mut open_ticks: Option<usize> = None;
    while i < chars.len() {
        if chars[i] == '\n' && open_ticks.is_some() {
            panic!("split backtick span at cols={cols}: {wrapped:?}");
        }
        if chars[i] == '`' {
            let mut run = 0usize;
            while i + run < chars.len() && chars[i + run] == '`' {
                run += 1;
            }
            if let Some(open) = open_ticks {
                if run == open {
                    open_ticks = None;
                }
            } else {
                let mut j = i + run;
                let mut found = false;
                while j < chars.len() {
                    if chars[j] == '`' {
                        let mut got = 0usize;
                        while j + got < chars.len() && chars[j + got] == '`' {
                            got += 1;
                        }
                        if got == run {
                            found = true;
                            break;
                        }
                        j += got;
                    } else {
                        j += 1;
                    }
                }
                if found {
                    open_ticks = Some(run);
                }
            }
            i += run;
            continue;
        }
        i += 1;
    }
}

#[test]
fn seeded_prose_property() {
    doctor_prefix_edge_case();
    osc8_single_char_uri_case();
    osc_title_with_spaces_case();
    double_wrap_ready_edge_case();

    let bank = [
        "alpha",
        "beta",
        "你好",
        "👨‍👩‍👧",
        "👍🏽",
        "1️⃣",
        "`code span`",
        "``double ticks``",
        "https://example.com/a/very/long/path",
        "\u{1b}[31mred\u{1b}[0m",
        "\u{1b}[31mabcdefghijklmnop\u{1b}[0m",
        "ab\u{1b}[1mcdefgh\u{1b}[0mijkl",
        "1\u{1b}[31m\u{fe0f}\u{20e3}\u{1b}[0m",
        "👨\u{1b}[32m\u{200d}👩\u{200d}👧\u{1b}[0m",
        "\u{1b}[32mhttps://example.com/a/very/long/path\u{1b}[0m",
        "\u{1b}]0;my title\u{7}",
        "\u{1b}]8;id=7;https://example.com/long\u{7}click here\u{1b}]8;;\u{7}",
    ];
    let prefixes = [
        ("", ""),
        ("warn  git identity  ", "                    "),
        ("info  packset-embed  ", "                    "),
    ];
    let mut seed = 0x1234_5678_9abc_def0u64;
    for cols in 1..=120usize {
        for round in 0..12u64 {
            let (mut first, mut cont) = prefixes[(round as usize) % prefixes.len()];
            let mut body = String::new();
            for n in 0..16usize {
                seed = xorshift64(seed ^ cols as u64 ^ round ^ n as u64);
                if n > 0 {
                    body.push(if seed & 1 == 0 { ' ' } else { '\t' });
                }
                let idx = ((seed >> 3) as usize) % bank.len();
                body.push_str(bank[idx]);
            }
            if !first.is_empty() && cols < prose_display_width(first.trim_end()) {
                first = "";
                cont = "";
            }
            let wrapped = if first.is_empty() {
                wrap_prose(&body, Some(cols))
            } else {
                wrap_hanging_prose(&body, cols, first, cont)
            };
            let expected = if first.is_empty() {
                body.clone()
            } else {
                format!("{first}{body}")
            };
            assert_invariants(&expected, &wrapped, cols);
        }
    }
}

fn doctor_prefix_edge_case() {
    let doctor = "warn\tgit identity\tgit has no author for a commit";
    let doctor_wrapped = wrap_prose(doctor, Some(20));
    assert!(
        !doctor_wrapped.contains("warn  git identity  git"),
        "{doctor_wrapped}"
    );
    assert_invariants(
        "warn  git identity  git has no author for a commit",
        &doctor_wrapped,
        20,
    );
}

fn osc8_single_char_uri_case() {
    let osc8 = "See \u{1b}]8;;/\u{7}slash link text\u{1b}]8;;\u{7} now";
    let osc8_wrapped = wrap_prose(osc8, Some(18));
    assert!(
        osc8_wrapped.contains("\u{1b}]8;;/\u{7}slash link text\u{1b}]8;;\u{7}"),
        "{osc8_wrapped}"
    );
    assert_invariants(osc8, &osc8_wrapped, 18);
}

fn osc_title_with_spaces_case() {
    let osc_title = "pre \u{1b}]0;my title\u{7} post words";
    let osc_title_wrapped = wrap_prose(osc_title, Some(10));
    assert!(
        osc_title_wrapped.contains("\u{1b}]0;my title\u{7}"),
        "{osc_title_wrapped}"
    );
    assert_invariants(osc_title, &osc_title_wrapped, 10);
}

fn double_wrap_ready_edge_case() {
    let once = wrap_hanging_prose(
        "This is a deliberately long title to verify single-pass wrapping at narrow widths",
        28,
        "probe-jqup  STARTED  [#C]  ",
        "                           ",
    );
    let twice = wrap_prose(&once, Some(28));
    for line in twice.lines() {
        assert!(prose_display_width(line) <= 28, "{twice}");
    }
}

#[test]
fn property_case_doctor_prefix_edge() {
    doctor_prefix_edge_case();
}

#[test]
fn property_case_osc8_single_char_uri() {
    osc8_single_char_uri_case();
}

#[test]
fn property_case_osc_title_with_spaces() {
    osc_title_with_spaces_case();
}

#[test]
fn property_case_double_wrap_ready_edge() {
    double_wrap_ready_edge_case();
}
