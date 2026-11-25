//! Workflow render diagnostics for the shared public-text screen.
//!
//! This module does not classify text. It gives workflow callers line-numbered
//! diagnostics for [`crate::public_text::scan`], the same classifier the
//! handbook export calls.

use crate::public_text::{self, Class, Kind, Line, Screen};

/// Return a human-readable diagnostic for every refused line in `text`.
#[must_use]
pub fn scan_public(text: &str) -> Vec<String> {
    scan_public_with_screen(text, &public_screen())
}

pub(crate) fn scan_public_with_screen(text: &str, screen: &Screen) -> Vec<String> {
    scan_with_kinds(
        text,
        1,
        |line| {
            let lower = line.trim_start().to_ascii_lowercase();
            let property = [
                ":properties:",
                ":end:",
                ":step:",
                ":depends:",
                ":kind:",
                ":role:",
                ":quorum:",
                ":deed:",
            ]
            .iter()
            .any(|prefix| lower.starts_with(prefix));
            if lower.starts_with("#+title:")
                || lower.starts_with("#+version:")
                || lower.starts_with("* ")
                || property
            {
                Kind::Structure
            } else {
                Kind::Text
            }
        },
        screen,
    )
}

/// Scan author-controlled text, assigning its first line `first_line`.
#[must_use]
pub fn scan_author(text: &str, first_line: usize) -> Vec<String> {
    scan_text(text, first_line, Kind::Text)
}

/// Scan a structural step key before the renderer writes it.
#[must_use]
pub fn scan_step_key(text: &str, line: usize) -> Vec<String> {
    scan_text(text, line, Kind::Id)
}

/// Scan a `:ROLE:` value.
#[must_use]
pub fn scan_role(text: &str, line: usize) -> Vec<String> {
    scan_text(text, line, Kind::Text)
}

/// Scan body text with the shared public-text classifier.
#[must_use]
pub fn scan_body(text: &str, first_line: usize) -> Vec<String> {
    scan_text(text, first_line, Kind::Text)
}

pub(crate) fn scan_body_with_screen(text: &str, first_line: usize, screen: &Screen) -> Vec<String> {
    scan_with_kinds(text, first_line, |_| Kind::Text, screen)
}

pub(crate) fn scan_role_with_screen(text: &str, first_line: usize, screen: &Screen) -> Vec<String> {
    scan_with_kinds(text, first_line, |_| Kind::Text, screen)
}

fn scan_text(text: &str, first_line: usize, kind: Kind) -> Vec<String> {
    scan_with_kinds(text, first_line, |_| kind, &public_screen())
}

fn scan_with_kinds(
    text: &str,
    first_line: usize,
    kind: impl Fn(&str) -> Kind,
    screen: &Screen,
) -> Vec<String> {
    let source: Vec<(usize, &str)> = text
        .split('\n')
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty())
        .collect();
    let lines: Vec<Line<'_>> = source
        .iter()
        .map(|(_, text)| Line {
            text,
            kind: kind(text),
        })
        .collect();
    public_text::scan(&lines, screen)
        .into_iter()
        .zip(source)
        .filter_map(|(class, (offset, _))| {
            class.map(|class| {
                format!(
                    "line {} contains {}",
                    first_line + offset,
                    diagnostic(class)
                )
            })
        })
        .collect()
}

fn diagnostic(class: Class) -> &'static str {
    match class {
        Class::Term => "tool or model name",
        Class::Seat => "seat name",
        Class::Path => "private path",
        Class::Secret => "secret",
        Class::Address => "address",
        Class::Mailbox => "email",
        Class::Org => "Org structure",
        Class::Character => "character outside the public set",
    }
}

pub(crate) fn public_screen() -> Screen {
    Screen {
        seats: crate::tracker_roster_names()
            .into_iter()
            .map(|seat| seat.trim().to_string())
            .filter(|seat| {
                seat.chars().count() >= 2
                    && seat
                        .chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
            })
            .collect(),
        internal: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostics_are_the_shared_classification() {
        for (text, class) in [
            ("/home/alice/notes.org", Class::Path),
            ("the password is hunter2", Class::Secret),
            ("Claude wrote this", Class::Term),
        ] {
            assert_eq!(
                !scan_body(text, 1).is_empty(),
                public_text::classify(text, &Screen::default()) == Some(class),
                "{text:?}"
            );
        }
    }
}
