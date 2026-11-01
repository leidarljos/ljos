//! A person's yes, taken from their own words.
//!
//! A lesson an agent proposes waits for the person to accept it. Until
//! now that meant a terminal: `ljos accept ID` with no agent runner above
//! the process, since a runner could otherwise accept its own proposal
//! and call it reviewed. A person who said yes in chat had no way to have
//! that yes count, and the proposal sat open until they opened a terminal
//! of their own.
//!
//! A quoted yes is the other way in. The seat that heard the words passes
//! them on: what the person said, when they said it, and which seat
//! carried it. Nothing here proves the words; the record is the point. An
//! acceptance that came through a seat says so, keeps the sentence it
//! rests on, and can be read back and disowned. The terminal path is
//! unchanged, and still needs no quote.

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// The fewest words a quote may have.
///
/// Four, the floor [`crate::capture`] holds a quoted prompt to: "yes" and
/// "go ahead" are agreement with nothing in particular, and a sentence
/// that names what is being agreed to is worth keeping.
pub const MIN_WORDS: usize = 4;

/// The longest quote kept, in characters. A person's yes is a sentence or
/// a paragraph; a whole transcript pasted in is not a yes.
pub const MAX_CHARS: usize = 2000;

/// The longest seat name `--via` takes.
pub const VIA_MAX: usize = 32;

/// How far ahead of now `--at` may be. A relay's clock can sit a little
/// ahead of this one; a yes dated tomorrow is not a yes anyone gave.
pub const AHEAD: i64 = 300;

/// A person's yes as a seat carried it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Yes {
    /// What the person said, in their words.
    pub quote: String,
    /// When they said it, RFC 3339, in the offset it was said in.
    pub at: String,
    /// The seat that carried the words here.
    pub via: String,
}

impl Yes {
    /// The line a person reads back: the seat, the time and the words the
    /// acceptance rests on.
    #[must_use]
    pub fn line(&self) -> String {
        format!(
            "on {}'s word at {}: \"{}\"\n",
            self.via,
            self.at,
            self.quote.split_whitespace().collect::<Vec<_>>().join(" ")
        )
    }
}

/// The yes `--quote`, `--at` and `--via` name, or `None` when none of the
/// three was given and this is the terminal path.
///
/// # Errors
///
/// One or two of the three given: a quote with no time and no seat is not
/// a record of anything. An empty or short quote, a quote past
/// [`MAX_CHARS`], a time that is not RFC 3339 or is ahead of now, or a
/// seat name with whitespace or past [`VIA_MAX`].
pub fn read(quote: Option<&str>, at: Option<&str>, via: Option<&str>) -> Result<Option<Yes>> {
    let given = [quote.is_some(), at.is_some(), via.is_some()];
    if given.iter().all(|had| !had) {
        return Ok(None);
    }
    if !given.iter().all(|had| *had) {
        let missing: Vec<&str> = [
            ("--quote", quote.is_some()),
            ("--at", at.is_some()),
            ("--via", via.is_some()),
        ]
        .into_iter()
        .filter(|(_, had)| !had)
        .map(|(flag, _)| flag)
        .collect();
        bail!(
            "accept: a quoted yes needs {}, so the record says what was said, when, and which seat carried it. Nothing was written",
            missing.join(" and ")
        );
    }
    let quote = quote.unwrap_or_default().trim();
    let at = at.unwrap_or_default().trim();
    let via = via.unwrap_or_default().trim();
    Ok(Some(Yes {
        quote: quote_words(quote)?,
        at: said_at(at)?,
        via: carrier(via)?,
    }))
}

fn quote_words(quote: &str) -> Result<String> {
    if quote.is_empty() {
        bail!("accept: --quote is empty, and an empty quote is not a yes. Nothing was written");
    }
    if quote.chars().count() > MAX_CHARS {
        bail!(
            "accept: --quote is longer than {MAX_CHARS} characters. Quote the sentence that says yes, not the conversation. Nothing was written"
        );
    }
    let words = quote.split_whitespace().count();
    if words < MIN_WORDS {
        bail!(
            "accept: --quote is {words} words, and a yes is kept from {MIN_WORDS} or more, so the record names what was agreed to. Nothing was written"
        );
    }
    Ok(quote.to_string())
}

fn said_at(at: &str) -> Result<String> {
    let when = chrono::DateTime::parse_from_rfc3339(at).map_err(|e| {
        anyhow::anyhow!(
            "accept: --at {at:?} is not RFC 3339 ({e}), such as 2026-10-28T14:02:11Z. Nothing was written"
        )
    })?;
    let ahead = when.timestamp() - chrono::Utc::now().timestamp();
    if ahead > AHEAD {
        bail!(
            "accept: --at {at:?} is ahead of now, so nobody has said it yet. Nothing was written"
        );
    }
    Ok(at.to_string())
}

fn carrier(via: &str) -> Result<String> {
    if via.is_empty() {
        bail!("accept: --via is empty, and a yes is carried by a named seat. Nothing was written");
    }
    if via.chars().any(char::is_whitespace) || via.chars().count() > VIA_MAX {
        bail!(
            "accept: --via {via:?} is not a seat name: one word, {VIA_MAX} characters or fewer. Nothing was written"
        );
    }
    Ok(via.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORDS: &str = "yes, write that lesson about uv";

    fn an_hour_ago() -> String {
        crate::utc_at(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                - 3600,
        )
    }

    #[test]
    fn a_yes_keeps_the_words_the_time_and_the_seat() {
        let at = an_hour_ago();
        let yes = read(Some(WORDS), Some(&at), Some("inky"))
            .unwrap()
            .expect("a yes");
        assert_eq!(yes.quote, WORDS);
        assert_eq!(yes.at, at);
        assert_eq!(yes.via, "inky");
        let line = yes.line();
        assert!(line.contains("inky"), "{line}");
        assert!(line.contains(WORDS), "{line}");
    }

    #[test]
    fn the_terminal_path_names_no_yes() {
        assert_eq!(read(None, None, None).unwrap(), None);
    }

    #[test]
    fn a_half_recorded_yes_is_refused() {
        let at = an_hour_ago();
        let err = read(Some(WORDS), None, None).unwrap_err().to_string();
        assert!(err.contains("--at and --via"), "{err}");
        assert!(err.contains("Nothing was written"), "{err}");
        let err = read(None, Some(&at), Some("inky")).unwrap_err().to_string();
        assert!(err.contains("--quote"), "{err}");
    }

    #[test]
    fn a_bare_yes_is_not_kept_as_a_yes() {
        let at = an_hour_ago();
        let err = read(Some("yes"), Some(&at), Some("inky"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("1 words"), "{err}");
        let err = read(Some("   "), Some(&at), Some("inky"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("empty"), "{err}");
        let long = "word ".repeat(MAX_CHARS);
        let err = read(Some(&long), Some(&at), Some("inky"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("longer than"), "{err}");
    }

    #[test]
    fn a_time_nobody_has_reached_is_refused() {
        let soon = crate::utc_at(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0)
                + 86_400,
        );
        let err = read(Some(WORDS), Some(&soon), Some("inky"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("ahead of now"), "{err}");
        let err = read(Some(WORDS), Some("yesterday"), Some("inky"))
            .unwrap_err()
            .to_string();
        assert!(err.contains("RFC 3339"), "{err}");
    }

    #[test]
    fn a_carrier_is_one_seat_name() {
        let at = an_hour_ago();
        for bad in ["", "two seats", &"s".repeat(VIA_MAX + 1)] {
            assert!(
                read(Some(WORDS), Some(&at), Some(bad)).is_err(),
                "took {bad:?} as a seat"
            );
        }
    }
}
