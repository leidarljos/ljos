//! Strings the earlier unit tests of the public-text screen held fixed.
//! The screen has since been rebuilt, so they are kept as a second table.

use ljos_cli::public_text::{classify, Screen};

#[path = "common/public_probes.rs"]
mod public_probes;

use public_probes::{BENIGN, LEAKS};

#[test]
fn every_earlier_leak_is_still_refused() {
    let screen = Screen::default();
    let kept: Vec<&str> = LEAKS
        .iter()
        .copied()
        .filter(|text| classify(text, &screen).is_none())
        .collect();
    assert!(kept.is_empty(), "{} not refused:\n{kept:#?}", kept.len());
}

#[test]
fn every_earlier_ordinary_string_still_passes() {
    let screen = Screen::default();
    let refused: Vec<&str> = BENIGN
        .iter()
        .copied()
        .filter(|text| classify(text, &screen).is_some())
        .collect();
    assert!(
        refused.is_empty(),
        "{} refused:\n{refused:#?}",
        refused.len()
    );
}
