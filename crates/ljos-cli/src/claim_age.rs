//! How long a claim has been held, counted from the two instants.
//!
//! The tracker stores the claim as an org stamp in local wall time. Taking
//! the calendar date out of that stamp and subtracting it from a UTC date
//! is one day off when local midnight and UTC midnight are not the same
//! moment, so a claim from earlier the same day prints as `-1d`. A clock a
//! few minutes ahead makes the same negative number. The age is the elapsed
//! time. A stamp ahead of now, or younger than a minute, is `just now`.

use chrono::{DateTime, FixedOffset, LocalResult, NaiveDateTime, Offset, TimeZone};

/// `just now`, `12m`, `3h`, or `4d`.
///
/// Under a minute, and any instant ahead of `now`, is `just now`. A minute
/// through 59 is `Nm`. An hour through 23 is `Nh`. A day or more is whole
/// days, `Nd`. The text never starts with a minus.
#[must_use]
pub fn between(then: DateTime<FixedOffset>, now: DateTime<FixedOffset>) -> String {
    let secs = now.signed_duration_since(then).num_seconds();
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
pub fn instant(stamp: &str, now: DateTime<FixedOffset>) -> Option<DateTime<FixedOffset>> {
    let offset = now.offset().fix();
    let text = stamp
        .trim()
        .trim_matches(|c: char| matches!(c, '[' | '<' | ']' | '>'));
    let mut parts = text.split_whitespace();
    let date = parts.next()?;
    let clock = parts.find(|part| part.contains(':')).unwrap_or("00:00");
    let clock = clock.get(..5).unwrap_or(clock);
    let naive = NaiveDateTime::parse_from_str(&format!("{date} {clock}"), "%Y-%m-%d %H:%M").ok()?;
    match offset.from_local_datetime(&naive) {
        LocalResult::Single(at) | LocalResult::Ambiguous(at, _) => Some(at),
        LocalResult::None => None,
    }
}

/// Age of an org stamp at `now`. `None` when the stamp does not parse.
#[must_use]
pub fn at(stamp: &str, now: DateTime<FixedOffset>) -> Option<String> {
    Some(between(instant(stamp, now)?, now))
}

/// Whole days between the instants. `Some(0)` when the stamp is ahead of
/// now. `None` when the stamp does not parse. Never negative.
#[must_use]
pub fn days(stamp: &str, now: DateTime<FixedOffset>) -> Option<i64> {
    let then = instant(stamp, now)?;
    let secs = now.signed_duration_since(then).num_seconds();
    Some(if secs <= 0 { 0 } else { secs / 86_400 })
}

/// `claimed 12m by ada`, or `claimed by ada` when the stamp does not parse.
#[must_use]
pub fn phrase(who: &str, stamp: Option<&str>, now: DateTime<FixedOffset>) -> String {
    match stamp.and_then(|stamp| at(stamp, now)) {
        Some(age) => format!("claimed {age} by {who}"),
        None => format!("claimed by {who}"),
    }
}

/// The ready-line suffix, `  (claimed 12m by ada)`. Empty when `who` is blank.
#[must_use]
pub fn suffix(who: Option<&str>, stamp: Option<&str>, now: DateTime<FixedOffset>) -> String {
    let Some(who) = who.map(str::trim).filter(|who| !who.is_empty()) else {
        return String::new();
    };
    format!("  ({})", phrase(who, stamp, now))
}

/// Now in this machine's zone, as a fixed offset the age math can share
/// with a stamp.
///
/// `LJOS_CLAIM_NOW`, an RFC 3339 instant, is now instead, so a test that
/// prints a claim age does not depend on how long its runs take. A value
/// that does not parse is ignored.
#[must_use]
pub fn now_here() -> DateTime<FixedOffset> {
    now_from(std::env::var("LJOS_CLAIM_NOW").ok().as_deref())
}

/// `raw` as an instant in this machine's zone, or the clock when it does
/// not parse. Org stamps are local wall time, so the instant must carry
/// the local offset, whatever offset `raw` was written in.
fn now_from(raw: Option<&str>) -> DateTime<FixedOffset> {
    raw.and_then(|raw| DateTime::parse_from_rfc3339(raw.trim()).ok())
        .map(|at| at.with_timezone(&chrono::Local).fixed_offset())
        .unwrap_or_else(|| chrono::Local::now().fixed_offset())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at_offset(
        hours_east: i32,
        y: i32,
        m: u32,
        d: u32,
        hh: u32,
        mm: u32,
    ) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(hours_east * 3600)
            .unwrap()
            .with_ymd_and_hms(y, m, d, hh, mm, 0)
            .unwrap()
    }

    /// Local midnight has passed and the UTC date is still yesterday.
    /// Subtracting those two calendar dates is `-1`. The claim was twelve
    /// minutes ago.
    #[test]
    fn a_claim_minutes_ago_across_the_utc_date_is_minutes() {
        let now = at_offset(2, 2026, 11, 16, 0, 20);
        assert_eq!(
            now.with_timezone(&chrono::Utc).date_naive().to_string(),
            "2026-11-15"
        );
        let stamp = "[2026-11-16 Mon 00:08]";
        let age = at(stamp, now).unwrap();
        assert_eq!(age, "12m");
        assert!(!age.starts_with('-'), "{age}");
        let utc_day = now.with_timezone(&chrono::Utc).date_naive();
        let local_day = chrono::NaiveDate::from_ymd_opt(2026, 11, 16).unwrap();
        assert_eq!((utc_day - local_day).num_days(), -1);
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

    #[test]
    fn two_writings_of_one_instant_age_the_same() {
        let utc = now_from(Some("2026-11-21T03:15:30+00:00"));
        let east = now_from(Some(" 2026-11-21T05:15:30+02:00 "));
        assert_eq!(utc, east);
        assert_eq!(utc.offset(), east.offset());
        let here = chrono::Utc
            .with_ymd_and_hms(2026, 11, 21, 3, 15, 0)
            .unwrap()
            .with_timezone(&chrono::Local);
        let stamp = here.format("[%Y-%m-%d %a %H:%M]").to_string();
        assert_eq!(at(&stamp, utc).as_deref(), Some("just now"));
        assert_eq!(at(&stamp, east).as_deref(), Some("just now"));
        assert_eq!(days(&stamp, east), Some(0));
    }

    #[test]
    fn an_unparsable_instant_is_the_clock() {
        for raw in [None, Some(""), Some("  "), Some("2026-11-21"), Some("soon")] {
            let before = chrono::Local::now().fixed_offset();
            let now = now_from(raw);
            let after = chrono::Local::now().fixed_offset();
            assert!(before <= now && now <= after, "{raw:?}: {now}");
            assert_eq!(now.offset(), before.offset(), "{raw:?}");
        }
    }
}
