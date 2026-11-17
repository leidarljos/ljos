//! How long a claim has been held, counted from the two instants.
//!
//! The tracker stores the claim as an org stamp in local wall time. Taking
//! the calendar date out of that stamp and subtracting it from a UTC date
//! is one day off when local midnight and UTC midnight are not the same
//! moment, so a claim from earlier the same day prints as `-1d`. A clock a
//! few minutes ahead makes the same negative number. The age is the elapsed
//! time. A stamp ahead of now, or younger than a minute, is `just now`.

use crate::clock;

/// An instant and the offset from UTC the wall clock is reading, the pair
/// a stamp is read against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Moment {
    secs: i64,
    offset: i32,
}

/// `just now`, `12m`, `3h`, or `4d`.
///
/// Under a minute, and any instant ahead of `now`, is `just now`. A minute
/// through 59 is `Nm`. An hour through 23 is `Nh`. A day or more is whole
/// days, `Nd`. The text never starts with a minus.
#[must_use]
pub fn between(then: Moment, now: Moment) -> String {
    let secs = now.secs - then.secs;
    if secs < 60 {
        return "just now".to_string();
    }
    let secs = secs as u64;
    if secs < 3_600 {
        format!("{}m", secs / 60)
    } else if secs < 86_400 {
        format!("{}h", secs / 3_600)
    } else {
        format!("{}d", secs / 86_400)
    }
}

/// `stamp` as an instant in the offset `now` is using.
///
/// The stamp is `[YYYY-MM-DD Day HH:MM]`, the local wall time the tracker
/// writes. A date with no clock is that day's midnight. `None` when the
/// stamp is not that shape.
#[must_use]
pub fn instant(stamp: &str, now: Moment) -> Option<Moment> {
    let text = stamp
        .trim()
        .trim_matches(|c: char| matches!(c, '[' | '<' | ']' | '>'));
    let mut parts = text.split_whitespace();
    let date = parts.next()?;
    let clock = parts.find(|part| part.contains(':')).unwrap_or("00:00");
    let clock = clock.get(..5).unwrap_or(clock);
    let naive = clock::parse_day_minute(&format!("{date} {clock}"))?;
    let secs = clock::local_epoch(naive, &clock::fixed_zone(now.offset)?)?;
    Some(Moment {
        secs,
        offset: now.offset,
    })
}

/// Age of an org stamp at `now`. `None` when the stamp does not parse.
#[must_use]
pub fn at(stamp: &str, now: Moment) -> Option<String> {
    Some(between(instant(stamp, now)?, now))
}

/// Whole days between the instants. `Some(0)` when the stamp is ahead of
/// now. `None` when the stamp does not parse. Never negative.
#[must_use]
pub fn days(stamp: &str, now: Moment) -> Option<i64> {
    let then = instant(stamp, now)?;
    let secs = now.secs - then.secs;
    Some(if secs <= 0 { 0 } else { secs / 86_400 })
}

/// `claimed 12m by ada`, or `claimed by ada` when the stamp does not parse.
#[must_use]
pub fn phrase(who: &str, stamp: Option<&str>, now: Moment) -> String {
    match stamp.and_then(|stamp| at(stamp, now)) {
        Some(age) => format!("claimed {age} by {who}"),
        None => format!("claimed by {who}"),
    }
}

/// The ready-line suffix, `  (claimed 12m by ada)`. Empty when `who` is blank.
#[must_use]
pub fn suffix(who: Option<&str>, stamp: Option<&str>, now: Moment) -> String {
    let Some(who) = who.map(str::trim).filter(|who| !who.is_empty()) else {
        return String::new();
    };
    format!("  ({})", phrase(who, stamp, now))
}

/// Now in this machine's zone, with the offset the age math shares with
/// a stamp.
#[must_use]
pub fn now_here() -> Moment {
    let secs = clock::Instant::now().timestamp();
    Moment {
        secs,
        offset: i32::try_from(clock::offset_secs_at(&clock::local_zone(), secs)).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use jiff::civil::date;

    fn at_offset(hours_east: i32, y: i16, m: i8, d: i8, hh: i8, mm: i8) -> Moment {
        let offset = hours_east * 3600;
        let zone = clock::fixed_zone(offset).unwrap();
        let secs = clock::local_epoch(date(y, m, d).at(hh, mm, 0, 0), &zone).unwrap();
        Moment { secs, offset }
    }

    /// Local midnight has passed and the UTC date is still yesterday.
    /// Subtracting those two calendar dates is `-1`. The claim was twelve
    /// minutes ago.
    #[test]
    fn a_claim_minutes_ago_across_the_utc_date_is_minutes() {
        let now = at_offset(2, 2026, 11, 16, 0, 20);
        assert_eq!(
            clock::utc_wall(clock::Instant::from_unix(now.secs))
                .unwrap()
                .day(),
            "2026-11-15"
        );
        let stamp = "[2026-11-16 Mon 00:08]";
        let age = at(stamp, now).unwrap();
        assert_eq!(age, "12m");
        assert!(!age.starts_with('-'), "{age}");
        let utc_day = clock::utc_wall(clock::Instant::from_unix(now.secs))
            .unwrap()
            .date();
        let local_day = date(2026, 11, 16);
        assert_eq!(clock::days_between(local_day, utc_day), -1);
        assert_eq!(days(stamp, now), Some(0));
        let suffix = suffix(Some("ada"), Some(stamp), now);
        assert_eq!(suffix, "  (claimed 12m by ada)");
        let raw = format!(
            "{:<22} {:<9} [#{}]  {}{}\n",
            "demo-kiln", "STARTED", "B", "Fire the kiln", suffix
        );
        let text = crate::scan::ready_text(&raw);
        assert!(text.contains("claimed 12m by ada"), "{text}");
        assert!(!text.contains("claimed -"), "{text}");
    }

    /// A clock a few minutes ahead must not print a negative age.
    #[test]
    fn a_stamp_a_few_minutes_ahead_is_just_now() {
        let now = at_offset(0, 2026, 11, 16, 12, 0);
        let stamp = "[2026-11-16 Mon 12:03]";
        assert_eq!(at(stamp, now).as_deref(), Some("just now"));
        assert_eq!(days(stamp, now), Some(0));
        let said = phrase("ada", Some(stamp), now);
        assert_eq!(said, "claimed just now by ada");
        assert!(!said.contains('-'), "{said}");
    }

    #[test]
    fn hours_and_days_count_from_the_instants() {
        let now = at_offset(0, 2026, 11, 16, 15, 0);
        assert_eq!(at("[2026-11-16 Mon 12:00]", now).as_deref(), Some("3h"));
        assert_eq!(at("[2026-11-14 Sat 15:00]", now).as_deref(), Some("2d"));
        assert_eq!(days("[2026-11-14 Sat 15:00]", now), Some(2));
    }
}
