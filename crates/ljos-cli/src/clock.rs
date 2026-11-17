//! Dates and times ljos reads, compares and prints, on `jiff`.
//!
//! Every text a person or a script sees keeps its exact shape: the stamps
//! the tracker writes, the times a sitting prints, the lesson days and the
//! JSON an agent reads. The parsers keep the grammar the `chrono` crate
//! gave them rather than the wider one `jiff` reads, so a line that was
//! refused stays refused and the message that names the refusal stays the
//! same. The calendar is `jiff`'s, years -9999 through 9999. An input in a
//! year outside it is refused where `chrono` read it, and a printed
//! instant outside it is left unprinted.

use std::cell::RefCell;
use std::fmt;
use std::fs;
use std::io::Read as _;
use std::time::SystemTime;

pub use jiff::civil::{Date, DateTime};
use jiff::tz::{AmbiguousOffset, Offset, TimeZone};
use jiff::{Span, Timestamp, Zoned};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    OutOfRange,
    Impossible,
    Invalid,
    TooShort,
    TooLong,
}

/// Why a text is not a time. The words are the ones `ljos` has always
/// printed after a refused `--at` or `--deadline`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParseError(Kind);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            Kind::OutOfRange => "input is out of range",
            Kind::Impossible => "no possible date and time matching input",
            Kind::Invalid => "input contains invalid characters",
            Kind::TooShort => "premature end of input",
            Kind::TooLong => "trailing input",
        })
    }
}

impl std::error::Error for ParseError {}

const OUT_OF_RANGE: ParseError = ParseError(Kind::OutOfRange);
const IMPOSSIBLE: ParseError = ParseError(Kind::Impossible);
const INVALID: ParseError = ParseError(Kind::Invalid);
const TOO_SHORT: ParseError = ParseError(Kind::TooShort);
const TOO_LONG: ParseError = ParseError(Kind::TooLong);

type Scan<'a, T> = Result<(&'a str, T), ParseError>;

const MIN_YEAR: i64 = -9999;
const MAX_YEAR: i64 = 9999;
const NANOS: u32 = 1_000_000_000;
const DAY_SECS: i64 = 86_400;

/// A point on the UTC timeline, as whole seconds since the epoch and the
/// nanoseconds after them. Nanoseconds from one billion up are a leap
/// second, the way RFC 3339 writes `23:59:60`. Instants order by seconds,
/// then nanoseconds, whatever offset a text gave.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Instant {
    secs: i64,
    frac: u32,
}

impl Instant {
    #[must_use]
    pub fn from_unix(secs: i64) -> Self {
        Self { secs, frac: 0 }
    }

    #[must_use]
    pub fn now() -> Self {
        Self::from(Timestamp::now())
    }

    #[must_use]
    pub fn from_system(at: SystemTime) -> Option<Self> {
        Timestamp::try_from(at).ok().map(Self::from)
    }

    /// This instant `secs` whole seconds on, or back when `secs` is negative.
    #[must_use]
    pub fn plus_secs(self, secs: i64) -> Self {
        Self {
            secs: self.secs.saturating_add(secs),
            frac: self.frac,
        }
    }

    /// Nanoseconds from `earlier` to this instant, negative when it is first.
    #[must_use]
    pub fn nanos_since(self, earlier: Self) -> i128 {
        let whole = i128::from(self.secs) - i128::from(earlier.secs);
        whole * i128::from(NANOS) + i128::from(self.frac) - i128::from(earlier.frac)
    }

    /// Whole seconds since the epoch, a leap second counted as the second
    /// before it.
    #[must_use]
    pub fn timestamp(self) -> i64 {
        self.secs
    }

    /// The whole second as a `jiff` instant, when it falls in `jiff`'s years.
    #[must_use]
    pub fn to_jiff(self) -> Option<Timestamp> {
        Timestamp::from_second(self.secs).ok()
    }
}

impl From<Timestamp> for Instant {
    fn from(at: Timestamp) -> Self {
        let nanos = at.as_nanosecond();
        let per_sec = i128::from(NANOS);
        Self {
            secs: i64::try_from(nanos.div_euclid(per_sec)).unwrap_or(i64::MAX),
            frac: u32::try_from(nanos.rem_euclid(per_sec)).unwrap_or(0),
        }
    }
}

/// An instant read off a clock in some zone: the date, the time of day and
/// the offset from UTC. The `text` methods print what `chrono`'s formats
/// of the same names printed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wall {
    date: Date,
    hour: i8,
    minute: i8,
    second: i8,
    offset: i32,
}

impl Wall {
    fn at(zoned: &Zoned, frac: u32) -> Self {
        let dt = zoned.datetime();
        Self {
            date: dt.date(),
            hour: dt.hour(),
            minute: dt.minute(),
            second: dt.second() + i8::from(frac >= NANOS),
            offset: zoned.offset().seconds(),
        }
    }

    #[must_use]
    pub fn date(&self) -> Date {
        self.date
    }

    /// `2026-10-12`.
    #[must_use]
    pub fn day(&self) -> String {
        day_text(self.date)
    }

    /// `09:00`.
    #[must_use]
    pub fn clock(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// `2026-10-12 09:00`.
    #[must_use]
    pub fn minute(&self) -> String {
        format!("{} {}", self.day(), self.clock())
    }

    /// `2026-10-12 09:00:00`.
    #[must_use]
    pub fn second(&self) -> String {
        format!("{}:{:02}", self.minute(), self.second)
    }

    /// `2026-10-12T09:00`.
    #[must_use]
    pub fn iso_minute(&self) -> String {
        format!("{}T{}", self.day(), self.clock())
    }

    /// `2026-10-12T09:00:00`.
    #[must_use]
    pub fn iso_second(&self) -> String {
        format!("{}:{:02}", self.iso_minute(), self.second)
    }

    /// `[2026-10-12 Mon 09:00]`, the stamp the tracker writes into a logbook.
    #[must_use]
    pub fn org(&self) -> String {
        org_stamp(self.date, self.hour, self.minute)
    }

    /// `-05:00`, `+00:00` or `+05:30`. Seconds in an old offset round to
    /// the nearest minute.
    #[must_use]
    pub fn offset(&self) -> String {
        let sign = if self.offset < 0 { '-' } else { '+' };
        let minutes = (self.offset.abs() + 30) / 60;
        format!("{sign}{:02}:{:02}", minutes / 60, minutes % 60)
    }

    /// RFC 3339 to the second: `2026-10-12T14:00:00Z`, or the numeric
    /// offset in place of the `Z` when `zulu` is off or the offset is not zero.
    #[must_use]
    pub fn rfc3339(&self, zulu: bool) -> String {
        if zulu && self.offset == 0 {
            format!("{}Z", self.iso_second())
        } else {
            format!("{}{}", self.iso_second(), self.offset())
        }
    }
}

/// `at` on the UTC clock. `None` when `at` is outside `jiff`'s years.
#[must_use]
pub fn utc_wall(at: Instant) -> Option<Wall> {
    wall_in(at, &TimeZone::UTC)
}

/// `at` on the clock of `zone`. `None` when `at` is outside `jiff`'s years.
#[must_use]
pub fn wall_in(at: Instant, zone: &TimeZone) -> Option<Wall> {
    let zoned = at.to_jiff()?.to_zoned(zone.clone());
    Some(Wall::at(&zoned, at.frac))
}

/// Now on the UTC clock.
#[must_use]
pub fn now_utc() -> Wall {
    wall_in(Instant::now(), &TimeZone::UTC).unwrap_or_else(|| epoch_wall(&TimeZone::UTC))
}

/// Now on this machine's clock.
#[must_use]
pub fn now_local() -> Wall {
    let zone = local_zone();
    wall_in(Instant::now(), &zone).unwrap_or_else(|| epoch_wall(&zone))
}

fn epoch_wall(zone: &TimeZone) -> Wall {
    Wall::at(&Timestamp::UNIX_EPOCH.to_zoned(zone.clone()), 0)
}

/// Today in this machine's zone.
#[must_use]
pub fn local_today() -> Date {
    now_local().date
}

/// The day `at` falls on in this machine's zone.
#[must_use]
pub fn local_day_of(at: SystemTime) -> Option<Date> {
    wall_in(Instant::from_system(at)?, &local_zone()).map(|w| w.date)
}

/// `[2026-10-12 Mon 09:00]`: `day` at `hour:minute` in the tracker's stamp.
#[must_use]
pub fn org_stamp(day: Date, hour: i8, minute: i8) -> String {
    format!(
        "[{} {} {hour:02}:{minute:02}]",
        day_text(day),
        weekday_text(day)
    )
}

/// `2026-10-12`.
#[must_use]
pub fn day_text(date: Date) -> String {
    format!(
        "{}-{:02}-{:02}",
        year_text(date.year()),
        date.month(),
        date.day()
    )
}

fn year_text(year: i16) -> String {
    if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else {
        format!("{year:+05}")
    }
}

fn weekday_text(date: Date) -> &'static str {
    const NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    NAMES[usize::from(date.weekday().to_sunday_zero_offset().unsigned_abs())]
}

/// Whole days from `earlier` to `later`, negative when `later` comes first.
#[must_use]
pub fn days_between(earlier: Date, later: Date) -> i64 {
    earlier
        .until(later)
        .map_or(0, |span| i64::from(span.get_days()))
}

/// `day` moved back by `days`, or `None` past `jiff`'s years.
#[must_use]
pub fn days_before(day: Date, days: i64) -> Option<Date> {
    day.checked_sub(Span::new().try_days(days).ok()?).ok()
}

/// Seconds since the epoch at `hour:minute:second` UTC on `day`. `None`
/// when the time of day does not exist; a leap second is not one.
#[must_use]
pub fn epoch_secs(day: Date, hour: u32, minute: u32, second: u32) -> Option<i64> {
    if hour >= 24 || minute >= 60 || second >= 60 {
        return None;
    }
    Some(
        epoch_days(day) * DAY_SECS
            + i64::from(hour) * 3600
            + i64::from(minute) * 60
            + i64::from(second),
    )
}

fn epoch_days(day: Date) -> i64 {
    days_between(jiff::civil::date(1970, 1, 1), day)
}

fn number(s: &str, min: usize, max: usize) -> Scan<'_, i64> {
    let bytes = s.as_bytes();
    if bytes.len() < min {
        return Err(TOO_SHORT);
    }
    let mut n = 0i64;
    for (i, &c) in bytes.iter().take(max).enumerate() {
        if !c.is_ascii_digit() {
            if i < min {
                return Err(INVALID);
            }
            return Ok((&s[i..], n));
        }
        n = n
            .checked_mul(10)
            .and_then(|n| n.checked_add(i64::from(c - b'0')))
            .ok_or(OUT_OF_RANGE)?;
    }
    Ok((&s[max.min(bytes.len())..], n))
}

fn literal<'a>(s: &'a str, lit: &str) -> Result<&'a str, ParseError> {
    if s.len() < lit.len() {
        return Err(TOO_SHORT);
    }
    s.strip_prefix(lit).ok_or(INVALID)
}

fn year_field(s: &str) -> Scan<'_, i64> {
    let s = s.trim_start();
    let (rest, year) = if let Some(rest) = s.strip_prefix('-') {
        let (rest, v) = number(rest, 1, usize::MAX)?;
        (rest, 0i64.checked_sub(v).ok_or(OUT_OF_RANGE)?)
    } else if let Some(rest) = s.strip_prefix('+') {
        number(rest, 1, usize::MAX)?
    } else {
        number(s, 1, 4)?
    };
    i32::try_from(year).map_err(|_| OUT_OF_RANGE)?;
    Ok((rest, year))
}

fn ranged_field(s: &str, width: usize, low: i64, high: i64) -> Scan<'_, i64> {
    let (rest, v) = number(s.trim_start(), 1, width)?;
    if (low..=high).contains(&v) {
        Ok((rest, v))
    } else {
        Err(OUT_OF_RANGE)
    }
}

fn make_date(year: i64, month: i64, day: i64) -> Result<Date, ParseError> {
    if !(MIN_YEAR..=MAX_YEAR).contains(&year) {
        return Err(OUT_OF_RANGE);
    }
    let (Ok(year), Ok(month), Ok(day)) =
        (i16::try_from(year), i8::try_from(month), i8::try_from(day))
    else {
        return Err(OUT_OF_RANGE);
    };
    Date::new(year, month, day).map_err(|_| OUT_OF_RANGE)
}

fn day_fields(s: &str) -> Scan<'_, (i64, i64, i64)> {
    let (s, year) = year_field(s)?;
    let s = literal(s, "-")?;
    let (s, month) = ranged_field(s, 2, 1, 12)?;
    let s = literal(s, "-")?;
    let (s, day) = ranged_field(s, 2, 1, 31)?;
    Ok((s, (year, month, day)))
}

/// A day written `YYYY-MM-DD`. Like `chrono`, it reads a one digit month or
/// day, a sign on the year, and blanks before each number, and refuses a
/// day that is not on the calendar or any text after it.
#[must_use]
pub fn parse_day(s: &str) -> Option<Date> {
    let (rest, (year, month, day)) = day_fields(s).ok()?;
    if !rest.is_empty() {
        return None;
    }
    make_date(year, month, day).ok()
}

/// A day and a minute written `YYYY-MM-DD HH:MM`, with the same latitude
/// as [`parse_day`] and any blanks between the two.
#[must_use]
pub fn parse_day_minute(s: &str) -> Option<DateTime> {
    let (rest, (year, month, day)) = day_fields(s).ok()?;
    let rest = rest.trim_start();
    let (rest, hour) = ranged_field(rest, 2, 0, 23).ok()?;
    let rest = literal(rest, ":").ok()?;
    let (rest, minute) = ranged_field(rest, 2, 0, 59).ok()?;
    if !rest.is_empty() {
        return None;
    }
    let date = make_date(year, month, day).ok()?;
    Some(date.at(i8::try_from(hour).ok()?, i8::try_from(minute).ok()?, 0, 0))
}

/// `local` as a count of seconds on the clock of `zone`. When the clock
/// showed it twice, the later reading wins, the one after the clock was set
/// back, as a stamp has always been read here. `None` when the clock skipped
/// it.
#[must_use]
pub fn local_epoch(local: DateTime, zone: &TimeZone) -> Option<i64> {
    let ambiguous = zone.to_ambiguous_timestamp(local);
    if matches!(ambiguous.offset(), AmbiguousOffset::Gap { .. }) {
        return None;
    }
    ambiguous.later().ok().map(|at| at.as_second())
}

fn utc_instant(
    date: Date,
    hour: i64,
    minute: i64,
    second: i64,
    nanos: u32,
    offset: i32,
) -> Instant {
    let secs = epoch_days(date) * DAY_SECS + hour * 3600 + minute * 60 + second - i64::from(offset);
    Instant { secs, frac: nanos }
}

fn time_of_day(hour: i64, minute: i64, second: i64, nanos: u32) -> Result<(i64, u32), ParseError> {
    if hour >= 24 || minute >= 60 || second >= 61 {
        return Err(OUT_OF_RANGE);
    }
    match second {
        60 => Ok((59, NANOS + nanos)),
        _ => Ok((second, nanos)),
    }
}

fn nanosecond(s: &str) -> Scan<'_, u32> {
    let before = s.len();
    let (rest, v) = number(s, 1, 9)?;
    let digits = before - rest.len();
    let scaled = u32::try_from(v)
        .ok()
        .and_then(|v| v.checked_mul(10u32.pow(u32::try_from(9 - digits).unwrap_or(0))))
        .ok_or(OUT_OF_RANGE)?;
    Ok((
        rest.trim_start_matches(|c: char| c.is_ascii_digit()),
        scaled,
    ))
}

fn offset_digits(s: &str) -> Result<(u8, u8), ParseError> {
    match s.as_bytes() {
        [a, b, ..] => Ok((*a, *b)),
        _ => Err(TOO_SHORT),
    }
}

/// `+hh:mm`, `-hh:mm` or `Z`, and with `rfc2822` set `+hhmm` or `-hhmm`.
/// Returns the offset in seconds.
fn zone_offset(s: &str, rfc2822: bool) -> Scan<'_, i32> {
    if !rfc2822 {
        if let Some(b'Z' | b'z') = s.as_bytes().first() {
            return Ok((&s[1..], 0));
        }
    }
    let (negative, mut s) = match s.chars().next() {
        Some('+') => (false, &s[1..]),
        Some('-') => (true, &s[1..]),
        Some('\u{2212}') if !rfc2822 => (true, &s['\u{2212}'.len_utf8()..]),
        Some(_) => return Err(INVALID),
        None => return Err(TOO_SHORT),
    };
    let hours = match offset_digits(s)? {
        (h1 @ b'0'..=b'9', h2 @ b'0'..=b'9') => i32::from((h1 - b'0') * 10 + (h2 - b'0')),
        _ => return Err(INVALID),
    };
    s = &s[2..];
    if !rfc2822 {
        s = match s.as_bytes().first() {
            Some(b':') => &s[1..],
            Some(_) => return Err(INVALID),
            None => return Err(TOO_SHORT),
        };
    }
    let minutes = match offset_digits(s) {
        Ok((m1 @ b'0'..=b'5', m2 @ b'0'..=b'9')) => i32::from((m1 - b'0') * 10 + (m2 - b'0')),
        Ok((b'6'..=b'9', b'0'..=b'9')) => return Err(OUT_OF_RANGE),
        Ok(_) => return Err(INVALID),
        Err(_) => return Err(TOO_SHORT),
    };
    s = &s[2..];
    let seconds = hours * 3600 + minutes * 60;
    Ok((s, if negative { -seconds } else { seconds }))
}

fn check_offset(offset: i32) -> Result<i32, ParseError> {
    if offset.abs() < 86_400 {
        Ok(offset)
    } else {
        Err(OUT_OF_RANGE)
    }
}

fn digit(bytes: &[u8], at: usize) -> Result<i64, ParseError> {
    match bytes.get(at) {
        Some(b) if b.is_ascii_digit() => Ok(i64::from(b - b'0')),
        _ => Err(INVALID),
    }
}

fn pair(bytes: &[u8], at: usize) -> Result<i64, ParseError> {
    Ok(digit(bytes, at)? * 10 + digit(bytes, at + 1)?)
}

/// An RFC 3339 date-time such as `2026-10-28T14:02:11Z`. The `T` may be a
/// `t` or a space, the `Z` a `z`, a fraction may run to any length, and
/// `:60` is a leap second. The offset is `Z` or `+hh:mm` with hours up to 23.
///
/// # Errors
///
/// The text is not that shape, names a day or time that does not exist, or
/// has text after the offset.
pub fn parse_rfc3339(s: &str) -> Result<Instant, ParseError> {
    let bytes = s.as_bytes();
    if bytes.len() < 19 {
        return Err(TOO_SHORT);
    }
    let year = digit(bytes, 0)? * 1000 + digit(bytes, 1)? * 100 + pair(bytes, 2)?;
    if bytes[4] != b'-' {
        return Err(INVALID);
    }
    let month = pair(bytes, 5)?;
    if bytes[7] != b'-' {
        return Err(INVALID);
    }
    let day = pair(bytes, 8)?;
    let date = make_date(year, month, day)?;
    if !matches!(bytes[10], b't' | b'T' | b' ') {
        return Err(INVALID);
    }
    let hour = pair(bytes, 11)?;
    if bytes[13] != b':' {
        return Err(INVALID);
    }
    let minute = pair(bytes, 14)?;
    if bytes[16] != b':' {
        return Err(INVALID);
    }
    let second = pair(bytes, 17)?;
    let (rest, nanos) = if bytes.get(19) == Some(&b'.') {
        nanosecond(&s[20..])?
    } else {
        (&s[19..], 0)
    };
    let (second, nanos) = time_of_day(hour, minute, second, nanos)?;
    let (rest, offset) = zone_offset(rest, false)?;
    if !rest.is_empty() {
        return Err(TOO_LONG);
    }
    let offset = check_offset(offset)?;
    Ok(utc_instant(date, hour, minute, second, nanos, offset))
}

fn short_weekday(s: &str) -> Scan<'_, i64> {
    let b = s.as_bytes();
    if b.len() < 3 {
        return Err(TOO_SHORT);
    }
    let day = match (b[0] | 32, b[1] | 32, b[2] | 32) {
        (b'm', b'o', b'n') => 1,
        (b't', b'u', b'e') => 2,
        (b'w', b'e', b'd') => 3,
        (b't', b'h', b'u') => 4,
        (b'f', b'r', b'i') => 5,
        (b's', b'a', b't') => 6,
        (b's', b'u', b'n') => 0,
        _ => return Err(INVALID),
    };
    Ok((&s[3..], day))
}

fn short_month(s: &str) -> Scan<'_, i64> {
    let b = s.as_bytes();
    if b.len() < 3 {
        return Err(TOO_SHORT);
    }
    let month = match (b[0] | 32, b[1] | 32, b[2] | 32) {
        (b'j', b'a', b'n') => 1,
        (b'f', b'e', b'b') => 2,
        (b'm', b'a', b'r') => 3,
        (b'a', b'p', b'r') => 4,
        (b'm', b'a', b'y') => 5,
        (b'j', b'u', b'n') => 6,
        (b'j', b'u', b'l') => 7,
        (b'a', b'u', b'g') => 8,
        (b's', b'e', b'p') => 9,
        (b'o', b'c', b't') => 10,
        (b'n', b'o', b'v') => 11,
        (b'd', b'e', b'c') => 12,
        _ => return Err(INVALID),
    };
    Ok((&s[3..], month))
}

fn blank(s: &str) -> Result<&str, ParseError> {
    let rest = s.trim_start();
    if rest.len() < s.len() {
        Ok(rest)
    } else if s.is_empty() {
        Err(TOO_SHORT)
    } else {
        Err(INVALID)
    }
}

fn colon(s: &str) -> Result<&str, ParseError> {
    match s.as_bytes().first() {
        Some(b':') => Ok(&s[1..]),
        Some(_) => Err(INVALID),
        None => Err(TOO_SHORT),
    }
}

fn legacy_offset(s: &str) -> Scan<'_, i32> {
    let letters = s
        .as_bytes()
        .iter()
        .position(|c| !c.is_ascii_alphabetic())
        .unwrap_or(s.len());
    if letters == 0 {
        return zone_offset(s, true);
    }
    let name = &s.as_bytes()[..letters];
    let rest = &s[letters..];
    let is = |want: &[u8]| name.eq_ignore_ascii_case(want);
    let hours = if is(b"gmt") || is(b"ut") || is(b"z") {
        0
    } else if is(b"edt") {
        -4
    } else if is(b"est") || is(b"cdt") {
        -5
    } else if is(b"cst") || is(b"mdt") {
        -6
    } else if is(b"mst") || is(b"pdt") {
        -7
    } else if is(b"pst") {
        -8
    } else if name.len() == 1
        && matches!(name[0], b'a'..=b'i' | b'k'..=b'y' | b'A'..=b'I' | b'K'..=b'Y')
    {
        0
    } else {
        return Err(INVALID);
    };
    Ok((rest, hours * 3600))
}

fn comment(s: &str) -> Result<&str, ParseError> {
    enum State {
        Start,
        Next(usize),
        Escape(usize),
    }
    let s = s.trim_start();
    let mut state = State::Start;
    for (i, c) in s.bytes().enumerate() {
        state = match (state, c) {
            (State::Start, b'(') => State::Next(1),
            (State::Next(1), b')') => return Ok(&s[i + 1..]),
            (State::Next(depth), b'\\') => State::Escape(depth),
            (State::Next(depth), b'(') => State::Next(depth + 1),
            (State::Next(depth), b')') => State::Next(depth - 1),
            (State::Next(depth) | State::Escape(depth), _) => State::Next(depth),
            _ => return Err(INVALID),
        };
    }
    Err(TOO_SHORT)
}

/// An RFC 2822 date-time such as `Thu, 22 Oct 2026 12:00:00 +0000`, as the
/// `Date:` header of a mail has it. The weekday is optional and must match
/// the date, a two or three digit year is read as 19xx or 20xx, the seconds
/// are optional, the zone is `+hhmm`, a legacy name such as `GMT` or `EST`,
/// or a letter, and `(comments)` may follow it.
///
/// # Errors
///
/// The text is not that shape or names a day or time that does not exist.
pub fn parse_rfc2822(s: &str) -> Result<Instant, ParseError> {
    let mut s = s.trim_start();
    let mut weekday = None;
    if let Ok((rest, day)) = short_weekday(s) {
        s = rest.strip_prefix(',').ok_or(INVALID)?;
        weekday = Some(day);
    }
    s = s.trim_start();
    let (rest, day) = ranged_day_of_month(s)?;
    s = blank(rest)?;
    let (rest, month) = short_month(s)?;
    s = blank(rest)?;
    let before = s.len();
    let (rest, mut year) = number(s, 2, usize::MAX)?;
    let digits = before - rest.len();
    s = rest;
    match (digits, year) {
        (2, 0..=49) => year += 2000,
        (2, 50..=99) | (3, _) => year += 1900,
        _ => {}
    }
    if i32::try_from(year).is_err() {
        return Err(OUT_OF_RANGE);
    }
    s = blank(s)?;
    let (rest, hour) = number(s, 2, 2)?;
    if hour > 23 {
        return Err(OUT_OF_RANGE);
    }
    s = colon(rest.trim_start())?.trim_start();
    let (rest, minute) = number(s, 2, 2)?;
    if minute > 59 {
        return Err(OUT_OF_RANGE);
    }
    s = rest;
    let mut second = 0;
    if let Ok(after) = colon(s.trim_start()) {
        let (rest, value) = number(after, 2, 2)?;
        if value > 60 {
            return Err(OUT_OF_RANGE);
        }
        second = value;
        s = rest;
    }
    s = blank(s)?;
    let (rest, offset) = legacy_offset(s)?;
    s = rest;
    while let Ok(rest) = comment(s) {
        s = rest;
    }
    if !s.is_empty() {
        return Err(TOO_LONG);
    }
    let date = make_date(year, month, day)?;
    if let Some(day) = weekday {
        if i64::from(date.weekday().to_sunday_zero_offset()) != day {
            return Err(IMPOSSIBLE);
        }
    }
    let (second, nanos) = time_of_day(hour, minute, second, 0)?;
    let offset = check_offset(offset).map_err(|_| OUT_OF_RANGE)?;
    Ok(utc_instant(date, hour, minute, second, nanos, offset))
}

fn ranged_day_of_month(s: &str) -> Scan<'_, i64> {
    let (rest, day) = number(s, 1, 2)?;
    if (1..=31).contains(&day) {
        Ok((rest, day))
    } else {
        Err(OUT_OF_RANGE)
    }
}

struct Cache {
    zone: TimeZone,
    source: Source,
    checked: std::time::Instant,
}

#[derive(PartialEq, Eq)]
enum Source {
    Environment(String),
    LocalTime(Option<SystemTime>),
}

impl Source {
    fn read() -> Self {
        match std::env::var("TZ") {
            Ok(tz) => Self::Environment(tz),
            Err(_) => Self::LocalTime(
                fs::symlink_metadata("/etc/localtime")
                    .and_then(|meta| meta.modified())
                    .ok(),
            ),
        }
    }

    fn tz(&self) -> Option<&str> {
        match self {
            Self::Environment(tz) => Some(tz),
            Self::LocalTime(_) => None,
        }
    }
}

thread_local! {
    static LOCAL: RefCell<Option<Cache>> = const { RefCell::new(None) };
}

/// This machine's time zone, read the way a process reads it: `TZ` when it
/// is set, a zone name, a path or a POSIX rule, else `/etc/localtime`. A
/// zone that cannot be read falls back to the name the system keeps, then
/// to UTC. The answer is kept for a second, so a `TZ` set in a running
/// process takes effect on the next second.
#[must_use]
pub fn local_zone() -> TimeZone {
    LOCAL.with(|cell| {
        let mut cell = cell.borrow_mut();
        let cache = cell.get_or_insert_with(|| {
            let source = Source::read();
            Cache {
                zone: zone_for(source.tz()),
                source,
                checked: std::time::Instant::now(),
            }
        });
        if cache.checked.elapsed().as_secs() >= 1 {
            let source = Source::read();
            if source != cache.source {
                cache.zone = zone_for(source.tz());
            }
            cache.source = source;
            cache.checked = std::time::Instant::now();
        }
        cache.zone.clone()
    })
}

/// The zone a process with this `TZ` (`None` when it is unset) lives in.
#[must_use]
pub fn zone_for(tz: Option<&str>) -> TimeZone {
    from_tz_var(tz)
        .or_else(from_system_name)
        .unwrap_or(TimeZone::UTC)
}

fn tzif(name: &str, bytes: &[u8]) -> Option<TimeZone> {
    TimeZone::tzif(name, bytes).ok()
}

fn from_tz_var(tz: Option<&str>) -> Option<TimeZone> {
    let Some(tz) = tz else {
        return tzif("localtime", &fs::read("/etc/localtime").ok()?);
    };
    if tz.is_empty() {
        return Some(TimeZone::UTC);
    }
    if let Some(path) = tz.strip_prefix(':') {
        return read_zone_file(path);
    }
    if let Some(file) = open_zone_file(tz) {
        return read_zone(tz, file);
    }
    TimeZone::posix(tz.trim_matches(|c: char| c.is_ascii_whitespace())).ok()
}

const ZONE_DIRS: [&str; 4] = [
    "/usr/share/zoneinfo",
    "/share/zoneinfo",
    "/etc/zoneinfo",
    "/usr/share/lib/zoneinfo",
];

fn open_zone_file(path: &str) -> Option<fs::File> {
    let path = std::path::Path::new(path);
    if path.is_absolute() {
        return fs::File::open(path).ok();
    }
    ZONE_DIRS
        .iter()
        .find_map(|dir| fs::File::open(std::path::Path::new(dir).join(path)).ok())
}

fn read_zone(name: &str, mut file: fs::File) -> Option<TimeZone> {
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).ok()?;
    tzif(name, &bytes)
}

fn read_zone_file(path: &str) -> Option<TimeZone> {
    read_zone(path, open_zone_file(path)?)
}

fn from_system_name() -> Option<TimeZone> {
    let name = system_zone_name()?;
    let bytes = fs::read(format!("{}/{name}", ZONE_DIRS[0])).ok()?;
    tzif(&name, &bytes)
}

fn system_zone_name() -> Option<String> {
    const PREFIXES: [&str; 4] = [
        "/usr/share/zoneinfo/",
        "../usr/share/zoneinfo/",
        "/etc/zoneinfo/",
        "../etc/zoneinfo/",
    ];
    let linked = fs::read_link("/etc/localtime")
        .ok()
        .and_then(|target| target.into_os_string().into_string().ok())
        .and_then(|target| {
            PREFIXES
                .iter()
                .find_map(|prefix| target.strip_prefix(prefix).map(str::to_string))
        });
    linked.or_else(|| {
        fs::read_to_string("/etc/timezone")
            .ok()
            .map(|name| name.trim_end().to_string())
    })
}

/// The offset of `zone` from UTC, in seconds, at `secs` since the epoch.
/// Zero when `secs` is outside `jiff`'s years.
#[must_use]
pub fn offset_secs_at(zone: &TimeZone, secs: i64) -> i64 {
    Timestamp::from_second(secs).map_or(0, |at| i64::from(zone.to_offset(at).seconds()))
}

/// A fixed offset of `secs` seconds from UTC, `None` when it is a day or more.
#[must_use]
pub fn fixed_zone(secs: i32) -> Option<TimeZone> {
    if secs.unsigned_abs() >= 86_400 {
        return None;
    }
    Offset::from_seconds(secs).ok().map(TimeZone::fixed)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Read = Result<(i64, u32), &'static str>;

    fn shown(read: Result<Instant, ParseError>) -> Result<(i64, u32), String> {
        read.map(|at| (at.secs, at.frac)).map_err(|e| e.to_string())
    }

    fn at(text: &str) -> Instant {
        parse_rfc3339(text).unwrap()
    }

    #[rustfmt::skip]
    const RFC3339: &[(&str, Read)] = &[
    ("2026-10-12T14:00:00Z", Ok((1791813600, 0))),
    ("2026-10-12T09:00:00-05:00", Ok((1791813600, 0))),
    ("2026-10-12T14:00:00+00:00", Ok((1791813600, 0))),
    ("2026-10-12t14:00:00z", Ok((1791813600, 0))),
    ("2026-10-12 14:00:00Z", Ok((1791813600, 0))),
    ("2026-10-12T14:00:00.123456789Z", Ok((1791813600, 123456789))),
    ("2026-12-31T23:59:60Z", Ok((1798761599, 1000000000))),
    ("2026-12-31T23:59:60.5Z", Ok((1798761599, 1500000000))),
    ("2026-10-12T14:00:00", Err("premature end of input")),
    ("2026-10-12", Err("premature end of input")),
    ("", Err("premature end of input")),
    ("2026-10-12T14:00:00Z ", Err("trailing input")),
    (" 2026-10-12T14:00:00Z", Err("input contains invalid characters")),
    ("2026-13-12T14:00:00Z", Err("input is out of range")),
    ("2026-02-30T00:00:00Z", Err("input is out of range")),
    ("2026-10-12T24:00:00Z", Err("input is out of range")),
    ("2026-10-12T14:60:00Z", Err("input is out of range")),
    ("2026-10-12T14:00:61Z", Err("input is out of range")),
    ("2026-10-12T14:00:00+24:00", Err("input is out of range")),
    ("2026-10-12T14:00:00+0530", Err("input contains invalid characters")),
    ("2026-10-12T14:00:00+05", Err("premature end of input")),
    ("2026-10-12T14:00:00 Z", Err("input contains invalid characters")),
    ("26-10-12T14:00:00Z", Err("premature end of input")),
    ("2026-1-2T14:00:00Z", Err("premature end of input")),
    ("+2026-10-12T14:00:00Z", Err("input contains invalid characters")),
    ("2026-10-12T14:00:00.Z", Err("input contains invalid characters")),
    ("2026-10-12T14:00:00.1234567890Z", Ok((1791813600, 123456789))),
    ("2026-10-12T14:00:00-00:00", Ok((1791813600, 0))),
    ("2026-10-12T14:00:00+05:30", Ok((1791793800, 0))),
    ("2026-10-12T14:00:00+23:59", Ok((1791727260, 0))),
    ("0001-01-01T00:00:00Z", Ok((-62135596800, 0))),
    ("9999-12-31T00:00:00Z", Ok((253402214400, 0))),
    ("1969-12-31T23:59:59Z", Ok((-1, 0))),
    ("2024-02-29T12:00:00Z", Ok((1709208000, 0))),
    ("2026-02-29T12:00:00Z", Err("input is out of range")),
    ("2026-10-12T14:00:00+05:60", Err("input is out of range")),
    ("2026-10-12T14:00:00Zjunk", Err("trailing input")),
    ("2026/10/12T14:00:00Z", Err("input contains invalid characters")),
    ];

    #[rustfmt::skip]
    const RFC2822: &[(&str, Read)] = &[
    ("Thu, 22 Oct 2026 12:00:00 +0000", Ok((1792670400, 0))),
    ("22 Oct 2026 12:00:00 GMT", Ok((1792670400, 0))),
    ("Thu, 22 Oct 2026 12:00:00 -0500 (EST)", Ok((1792688400, 0))),
    ("Fri, 22 Oct 2026 12:00:00 +0000", Err("no possible date and time matching input")),
    ("Thu, 22 Oct 2026 12:00 +0000", Ok((1792670400, 0))),
    ("Thu, 22 Oct 26 12:00:00 +0000", Ok((1792670400, 0))),
    ("garbage", Err("input contains invalid characters")),
    ("", Err("premature end of input")),
    ("Thu, 22 Oct 2026 12:00:00", Err("premature end of input")),
    ("Thu, 22 Oct 2026 12:00:00 EST", Ok((1792688400, 0))),
    ("Thu, 22 Oct 2026 12:00:00 +0000 junk", Err("trailing input")),
    ("Thu, 22 Octo 2026 12:00:00 +0000", Err("input contains invalid characters")),
    ("thu, 22 oct 2026 12:00:00 +0000", Ok((1792670400, 0))),
    ];

    #[rustfmt::skip]
    const DAYS: &[(&str, Option<&str>)] = &[
    ("2026-10-12", Some("2026-10-12")),
    ("2026-1-2", Some("2026-01-02")),
    (" 2026-10-12", Some("2026-10-12")),
    ("2026-10-12 ", None),
    ("2026-10-12x", None),
    ("2026-02-29", None),
    ("2024-02-29", Some("2024-02-29")),
    ("0001-01-01", Some("0001-01-01")),
    ("+2026-10-12", Some("2026-10-12")),
    ("-0001-01-01", Some("-0001-01-01")),
    ("20261012", None),
    ("2026/10/12", None),
    ("", None),
    ("2026-10-", None),
    ("2026-10-32", None),
    ("2026-00-10", None),
    ("2026-10-00", None),
    ("12026-10-12", None),
    ("026-10-12", Some("0026-10-12")),
    ("0000-01-01", Some("0000-01-01")),
    ("9999-12-31", Some("9999-12-31")),
    ("  2026-  10-12", Some("2026-10-12")),
    ("2026- 1- 2", Some("2026-01-02")),
    ("\u{3000}2026-10-12", Some("2026-10-12")),
    ];

    #[rustfmt::skip]
    const DAY_MINUTES: &[(&str, Option<&str>)] = &[
    ("2026-10-12 09:00", Some("2026-10-12 09:00")),
    ("2026-10-12 9:5", Some("2026-10-12 09:05")),
    ("2026-10-12 24:00", None),
    ("2026-10-12 09:60", None),
    ("2026-10-12 09:00:00", None),
    ("2026-10-12  09:00", Some("2026-10-12 09:00")),
    ("2026-10-12", None),
    ("2026-10-12T09:00", None),
    ("2026-10-12 09", None),
    ("2026-10-12 +9:00", None),
    ("2026-10-12 09:00 ", None),
    ];

    #[rustfmt::skip]
    const YEARS: &[(i16, &str)] = &[
    (-10, "-0010-03-04"),
    (-1, "-0001-03-04"),
    (0, "0000-03-04"),
    (1, "0001-03-04"),
    (99, "0099-03-04"),
    (999, "0999-03-04"),
    (1000, "1000-03-04"),
    (9999, "9999-03-04"),
    ];

    /// Offset in seconds, instant, then the second, the tracker stamp, the
    /// offset and the RFC 3339 text that instant prints at that offset.
    #[rustfmt::skip]
    const FIXED: &[(i32, &str, &str, &str, &str, &str)] = &[
    (0, "2026-10-12T14:00:00Z", "2026-10-12 14:00:00", "[2026-10-12 Mon 14:00]", "+00:00", "2026-10-12T14:00:00+00:00"),
    (0, "2026-03-08T07:30:00Z", "2026-03-08 07:30:00", "[2026-03-08 Sun 07:30]", "+00:00", "2026-03-08T07:30:00+00:00"),
    (0, "2026-11-01T06:30:00Z", "2026-11-01 06:30:00", "[2026-11-01 Sun 06:30]", "+00:00", "2026-11-01T06:30:00+00:00"),
    (0, "1969-12-31T23:59:59Z", "1969-12-31 23:59:59", "[1969-12-31 Wed 23:59]", "+00:00", "1969-12-31T23:59:59+00:00"),
    (0, "0099-07-04T00:00:00Z", "0099-07-04 00:00:00", "[0099-07-04 Sat 00:00]", "+00:00", "0099-07-04T00:00:00+00:00"),
    (0, "2026-12-31T23:59:60Z", "2026-12-31 23:59:60", "[2026-12-31 Thu 23:59]", "+00:00", "2026-12-31T23:59:60+00:00"),
    (-18000, "2026-10-12T14:00:00Z", "2026-10-12 09:00:00", "[2026-10-12 Mon 09:00]", "-05:00", "2026-10-12T09:00:00-05:00"),
    (-18000, "2026-03-08T07:30:00Z", "2026-03-08 02:30:00", "[2026-03-08 Sun 02:30]", "-05:00", "2026-03-08T02:30:00-05:00"),
    (-18000, "2026-11-01T06:30:00Z", "2026-11-01 01:30:00", "[2026-11-01 Sun 01:30]", "-05:00", "2026-11-01T01:30:00-05:00"),
    (-18000, "1969-12-31T23:59:59Z", "1969-12-31 18:59:59", "[1969-12-31 Wed 18:59]", "-05:00", "1969-12-31T18:59:59-05:00"),
    (-18000, "0099-07-04T00:00:00Z", "0099-07-03 19:00:00", "[0099-07-03 Fri 19:00]", "-05:00", "0099-07-03T19:00:00-05:00"),
    (-18000, "2026-12-31T23:59:60Z", "2026-12-31 18:59:60", "[2026-12-31 Thu 18:59]", "-05:00", "2026-12-31T18:59:60-05:00"),
    (19800, "2026-10-12T14:00:00Z", "2026-10-12 19:30:00", "[2026-10-12 Mon 19:30]", "+05:30", "2026-10-12T19:30:00+05:30"),
    (19800, "2026-03-08T07:30:00Z", "2026-03-08 13:00:00", "[2026-03-08 Sun 13:00]", "+05:30", "2026-03-08T13:00:00+05:30"),
    (19800, "2026-11-01T06:30:00Z", "2026-11-01 12:00:00", "[2026-11-01 Sun 12:00]", "+05:30", "2026-11-01T12:00:00+05:30"),
    (19800, "1969-12-31T23:59:59Z", "1970-01-01 05:29:59", "[1970-01-01 Thu 05:29]", "+05:30", "1970-01-01T05:29:59+05:30"),
    (19800, "0099-07-04T00:00:00Z", "0099-07-04 05:30:00", "[0099-07-04 Sat 05:30]", "+05:30", "0099-07-04T05:30:00+05:30"),
    (19800, "2026-12-31T23:59:60Z", "2027-01-01 05:29:60", "[2027-01-01 Fri 05:29]", "+05:30", "2027-01-01T05:29:60+05:30"),
    (-17762, "2026-10-12T14:00:00Z", "2026-10-12 09:03:58", "[2026-10-12 Mon 09:03]", "-04:56", "2026-10-12T09:03:58-04:56"),
    (-17762, "2026-03-08T07:30:00Z", "2026-03-08 02:33:58", "[2026-03-08 Sun 02:33]", "-04:56", "2026-03-08T02:33:58-04:56"),
    (-17762, "2026-11-01T06:30:00Z", "2026-11-01 01:33:58", "[2026-11-01 Sun 01:33]", "-04:56", "2026-11-01T01:33:58-04:56"),
    (-17762, "1969-12-31T23:59:59Z", "1969-12-31 19:03:57", "[1969-12-31 Wed 19:03]", "-04:56", "1969-12-31T19:03:57-04:56"),
    (-17762, "0099-07-04T00:00:00Z", "0099-07-03 19:03:58", "[0099-07-03 Fri 19:03]", "-04:56", "0099-07-03T19:03:58-04:56"),
    (-17762, "2026-12-31T23:59:60Z", "2026-12-31 19:03:58", "[2026-12-31 Thu 19:03]", "-04:56", "2026-12-31T19:03:58-04:56"),
    (45900, "2026-10-12T14:00:00Z", "2026-10-13 02:45:00", "[2026-10-13 Tue 02:45]", "+12:45", "2026-10-13T02:45:00+12:45"),
    (45900, "2026-03-08T07:30:00Z", "2026-03-08 20:15:00", "[2026-03-08 Sun 20:15]", "+12:45", "2026-03-08T20:15:00+12:45"),
    (45900, "2026-11-01T06:30:00Z", "2026-11-01 19:15:00", "[2026-11-01 Sun 19:15]", "+12:45", "2026-11-01T19:15:00+12:45"),
    (45900, "1969-12-31T23:59:59Z", "1970-01-01 12:44:59", "[1970-01-01 Thu 12:44]", "+12:45", "1970-01-01T12:44:59+12:45"),
    (45900, "0099-07-04T00:00:00Z", "0099-07-04 12:45:00", "[0099-07-04 Sat 12:45]", "+12:45", "0099-07-04T12:45:00+12:45"),
    (45900, "2026-12-31T23:59:60Z", "2027-01-01 12:44:60", "[2027-01-01 Fri 12:44]", "+12:45", "2027-01-01T12:44:60+12:45"),
    ];

    /// `TZ`, instant, then the same four texts on that zone's clock.
    #[rustfmt::skip]
    const ZONES: &[(&str, &str, &str, &str, &str, &str)] = &[
    ("EST5EDT,M3.2.0,M11.1.0", "2026-10-12T14:00:00Z", "2026-10-12 10:00:00", "[2026-10-12 Mon 10:00]", "-04:00", "2026-10-12T10:00:00-04:00"),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-03-08T07:30:00Z", "2026-03-08 03:30:00", "[2026-03-08 Sun 03:30]", "-04:00", "2026-03-08T03:30:00-04:00"),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-11-01T06:30:00Z", "2026-11-01 01:30:00", "[2026-11-01 Sun 01:30]", "-05:00", "2026-11-01T01:30:00-05:00"),
    ("EST5EDT,M3.2.0,M11.1.0", "1969-12-31T23:59:59Z", "1969-12-31 18:59:59", "[1969-12-31 Wed 18:59]", "-05:00", "1969-12-31T18:59:59-05:00"),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-12-31T23:59:60Z", "2026-12-31 18:59:60", "[2026-12-31 Thu 18:59]", "-05:00", "2026-12-31T18:59:60-05:00"),
    ("IST-5:30", "2026-10-12T14:00:00Z", "2026-10-12 19:30:00", "[2026-10-12 Mon 19:30]", "+05:30", "2026-10-12T19:30:00+05:30"),
    ("IST-5:30", "2026-03-08T07:30:00Z", "2026-03-08 13:00:00", "[2026-03-08 Sun 13:00]", "+05:30", "2026-03-08T13:00:00+05:30"),
    ("IST-5:30", "2026-11-01T06:30:00Z", "2026-11-01 12:00:00", "[2026-11-01 Sun 12:00]", "+05:30", "2026-11-01T12:00:00+05:30"),
    ("IST-5:30", "1969-12-31T23:59:59Z", "1970-01-01 05:29:59", "[1970-01-01 Thu 05:29]", "+05:30", "1970-01-01T05:29:59+05:30"),
    ("IST-5:30", "2026-12-31T23:59:60Z", "2027-01-01 05:29:60", "[2027-01-01 Fri 05:29]", "+05:30", "2027-01-01T05:29:60+05:30"),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-10-12T14:00:00Z", "2026-10-12 16:00:00", "[2026-10-12 Mon 16:00]", "+02:00", "2026-10-12T16:00:00+02:00"),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-03-08T07:30:00Z", "2026-03-08 08:30:00", "[2026-03-08 Sun 08:30]", "+01:00", "2026-03-08T08:30:00+01:00"),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-11-01T06:30:00Z", "2026-11-01 07:30:00", "[2026-11-01 Sun 07:30]", "+01:00", "2026-11-01T07:30:00+01:00"),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "1969-12-31T23:59:59Z", "1970-01-01 00:59:59", "[1970-01-01 Thu 00:59]", "+01:00", "1970-01-01T00:59:59+01:00"),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-12-31T23:59:60Z", "2027-01-01 00:59:60", "[2027-01-01 Fri 00:59]", "+01:00", "2027-01-01T00:59:60+01:00"),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-10-12T14:00:00Z", "2026-10-13 01:00:00", "[2026-10-13 Tue 01:00]", "+11:00", "2026-10-13T01:00:00+11:00"),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-03-08T07:30:00Z", "2026-03-08 18:30:00", "[2026-03-08 Sun 18:30]", "+11:00", "2026-03-08T18:30:00+11:00"),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-11-01T06:30:00Z", "2026-11-01 17:30:00", "[2026-11-01 Sun 17:30]", "+11:00", "2026-11-01T17:30:00+11:00"),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "1969-12-31T23:59:59Z", "1970-01-01 10:59:59", "[1970-01-01 Thu 10:59]", "+11:00", "1970-01-01T10:59:59+11:00"),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-12-31T23:59:60Z", "2027-01-01 10:59:60", "[2027-01-01 Fri 10:59]", "+11:00", "2027-01-01T10:59:60+11:00"),
    ("UTC0", "2026-10-12T14:00:00Z", "2026-10-12 14:00:00", "[2026-10-12 Mon 14:00]", "+00:00", "2026-10-12T14:00:00+00:00"),
    ("UTC0", "2026-03-08T07:30:00Z", "2026-03-08 07:30:00", "[2026-03-08 Sun 07:30]", "+00:00", "2026-03-08T07:30:00+00:00"),
    ("UTC0", "2026-11-01T06:30:00Z", "2026-11-01 06:30:00", "[2026-11-01 Sun 06:30]", "+00:00", "2026-11-01T06:30:00+00:00"),
    ("UTC0", "1969-12-31T23:59:59Z", "1969-12-31 23:59:59", "[1969-12-31 Wed 23:59]", "+00:00", "1969-12-31T23:59:59+00:00"),
    ("UTC0", "2026-12-31T23:59:60Z", "2026-12-31 23:59:60", "[2026-12-31 Thu 23:59]", "+00:00", "2026-12-31T23:59:60+00:00"),
    ("", "2026-10-12T14:00:00Z", "2026-10-12 14:00:00", "[2026-10-12 Mon 14:00]", "+00:00", "2026-10-12T14:00:00+00:00"),
    ("", "2026-03-08T07:30:00Z", "2026-03-08 07:30:00", "[2026-03-08 Sun 07:30]", "+00:00", "2026-03-08T07:30:00+00:00"),
    ("", "2026-11-01T06:30:00Z", "2026-11-01 06:30:00", "[2026-11-01 Sun 06:30]", "+00:00", "2026-11-01T06:30:00+00:00"),
    ("", "1969-12-31T23:59:59Z", "1969-12-31 23:59:59", "[1969-12-31 Wed 23:59]", "+00:00", "1969-12-31T23:59:59+00:00"),
    ("", "2026-12-31T23:59:60Z", "2026-12-31 23:59:60", "[2026-12-31 Thu 23:59]", "+00:00", "2026-12-31T23:59:60+00:00"),
    ];

    /// `TZ`, a clock reading, then the Unix second it names there.
    #[rustfmt::skip]
    const LOCAL_EPOCHS: &[(&str, &str, Option<i64>)] = &[
    ("EST5EDT,M3.2.0,M11.1.0", "2026-11-01 01:30", Some(1793514600)),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-03-08 02:30", None),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-10-25 02:30", Some(1792909800)),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-03-29 02:30", Some(1774765800)),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-10-12 09:00", Some(1791810000)),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-04-05 02:15", Some(1775369700)),
    ("EST5EDT,M3.2.0,M11.1.0", "2026-10-04 02:15", Some(1791094500)),
    ("IST-5:30", "2026-11-01 01:30", Some(1793476800)),
    ("IST-5:30", "2026-03-08 02:30", Some(1772917200)),
    ("IST-5:30", "2026-10-25 02:30", Some(1792875600)),
    ("IST-5:30", "2026-03-29 02:30", Some(1774731600)),
    ("IST-5:30", "2026-10-12 09:00", Some(1791775800)),
    ("IST-5:30", "2026-04-05 02:15", Some(1775335500)),
    ("IST-5:30", "2026-10-04 02:15", Some(1791060300)),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-11-01 01:30", Some(1793493000)),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-03-08 02:30", Some(1772933400)),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-10-25 02:30", Some(1792891800)),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-03-29 02:30", None),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-10-12 09:00", Some(1791788400)),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-04-05 02:15", Some(1775348100)),
    ("CET-1CEST,M3.5.0,M10.5.0/3", "2026-10-04 02:15", Some(1791072900)),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-11-01 01:30", Some(1793457000)),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-03-08 02:30", Some(1772897400)),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-10-25 02:30", Some(1792855800)),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-03-29 02:30", Some(1774711800)),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-10-12 09:00", Some(1791756000)),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-04-05 02:15", Some(1775317500)),
    ("<+1030>-10:30<+11>-11,M10.1.0,M4.1.0", "2026-10-04 02:15", None),
    ("UTC0", "2026-11-01 01:30", Some(1793496600)),
    ("UTC0", "2026-03-08 02:30", Some(1772937000)),
    ("UTC0", "2026-10-25 02:30", Some(1792895400)),
    ("UTC0", "2026-03-29 02:30", Some(1774751400)),
    ("UTC0", "2026-10-12 09:00", Some(1791795600)),
    ("UTC0", "2026-04-05 02:15", Some(1775355300)),
    ("UTC0", "2026-10-04 02:15", Some(1791080100)),
    ("", "2026-11-01 01:30", Some(1793496600)),
    ("", "2026-03-08 02:30", Some(1772937000)),
    ("", "2026-10-25 02:30", Some(1792895400)),
    ("", "2026-03-29 02:30", Some(1774751400)),
    ("", "2026-10-12 09:00", Some(1791795600)),
    ("", "2026-04-05 02:15", Some(1775355300)),
    ("", "2026-10-04 02:15", Some(1791080100)),
    ];

    fn assert_texts(wall: &Wall, row: &(&str, &str, &str, &str), label: &str) {
        assert_eq!(wall.second(), row.0, "{label} second");
        assert_eq!(wall.org(), row.1, "{label} org");
        assert_eq!(wall.offset(), row.2, "{label} offset");
        assert_eq!(wall.rfc3339(false), row.3, "{label} rfc3339");
    }

    #[test]
    fn an_rfc3339_time_reads_and_is_refused_in_the_same_words() {
        for (text, want) in RFC3339 {
            assert_eq!(
                shown(parse_rfc3339(text)),
                want.map_err(str::to_string),
                "{text:?}"
            );
        }
    }

    #[test]
    fn an_rfc2822_time_reads_and_is_refused_in_the_same_words() {
        for (text, want) in RFC2822 {
            assert_eq!(
                shown(parse_rfc2822(text)),
                want.map_err(str::to_string),
                "{text:?}"
            );
        }
    }

    #[test]
    fn a_lesson_day_reads_the_way_the_tracker_always_read_it() {
        for (text, want) in DAYS {
            assert_eq!(parse_day(text).map(day_text).as_deref(), *want, "{text:?}");
        }
    }

    #[test]
    fn a_sitting_minute_reads_the_way_the_tracker_always_read_it() {
        for (text, want) in DAY_MINUTES {
            let got = parse_day_minute(text).map(|at| {
                format!(
                    "{} {:02}:{:02}",
                    day_text(at.date()),
                    at.hour(),
                    at.minute()
                )
            });
            assert_eq!(got.as_deref(), *want, "{text:?}");
        }
    }

    #[test]
    fn a_year_prints_four_digits_or_a_sign() {
        for (year, want) in YEARS {
            let day = jiff::civil::date(*year, 3, 4);
            assert_eq!(day_text(day), *want);
            assert_eq!(parse_day(want), Some(day), "{want}");
        }
    }

    #[test]
    fn years_past_the_calendar_are_refused_and_left_unprinted() {
        assert_eq!(parse_day("+10000-01-01"), None);
        assert_eq!(parse_day("-10000-01-01"), None);
        assert!(parse_rfc3339("+10000-01-01T00:00:00Z").is_err());
        assert!(utc_wall(Instant::from_unix(253_402_300_800)).is_none());
        assert!(utc_wall(Instant::from_unix(-62_135_596_800 * 10)).is_none());
    }

    #[test]
    fn a_zulu_time_prints_to_the_second_and_a_leap_second_keeps_its_sixty() {
        let wall = utc_wall(at("2026-10-12T14:00:00Z")).unwrap();
        assert_eq!(wall.day(), "2026-10-12");
        assert_eq!(wall.clock(), "14:00");
        assert_eq!(wall.minute(), "2026-10-12 14:00");
        assert_eq!(wall.second(), "2026-10-12 14:00:00");
        assert_eq!(wall.iso_minute(), "2026-10-12T14:00");
        assert_eq!(wall.iso_second(), "2026-10-12T14:00:00");
        assert_eq!(wall.rfc3339(true), "2026-10-12T14:00:00Z");
        assert_eq!(wall.rfc3339(false), "2026-10-12T14:00:00+00:00");
        assert_eq!(wall.org(), "[2026-10-12 Mon 14:00]");
        let leap = utc_wall(at("2026-12-31T23:59:60Z")).unwrap();
        assert_eq!(leap.rfc3339(true), "2026-12-31T23:59:60Z");
        assert_eq!(leap.second(), "2026-12-31 23:59:60");
    }

    #[test]
    fn instants_order_by_second_then_fraction() {
        assert!(at("2026-12-31T23:59:60Z") > at("2026-12-31T23:59:59.999999999Z"));
        assert!(at("2026-10-12T09:00:00-05:00") == at("2026-10-12T14:00:00Z"));
        assert!(at("2026-10-12T14:00:00.1Z") > at("2026-10-12T14:00:00Z"));
        assert_eq!(
            at("2026-10-12T14:00:01Z").nanos_since(at("2026-10-12T14:00:00.5Z")),
            500_000_000
        );
    }

    #[test]
    fn a_fixed_offset_prints_the_texts_a_person_reads() {
        for (offset, instant, second, org, zone_offset, rfc3339) in FIXED {
            let zone = fixed_zone(*offset).unwrap();
            let wall = wall_in(at(instant), &zone).unwrap();
            assert_texts(
                &wall,
                &(second, org, zone_offset, rfc3339),
                &format!("{offset} {instant}"),
            );
        }
    }

    #[test]
    fn a_zone_prints_the_texts_a_person_reads() {
        for (tz, instant, second, org, offset, rfc3339) in ZONES {
            let wall = wall_in(at(instant), &zone_for(Some(tz))).unwrap();
            assert_texts(
                &wall,
                &(second, org, offset, rfc3339),
                &format!("{tz:?} {instant}"),
            );
        }
    }

    #[test]
    fn a_clock_reading_names_the_second_it_names_on_the_zone() {
        for (tz, reading, want) in LOCAL_EPOCHS {
            let local = parse_day_minute(reading).unwrap();
            assert_eq!(
                local_epoch(local, &zone_for(Some(tz))),
                *want,
                "{tz:?} {reading}"
            );
        }
    }

    #[test]
    fn an_offset_of_a_day_or_more_is_no_zone() {
        assert!(fixed_zone(86_399).is_some());
        assert!(fixed_zone(-86_399).is_some());
        assert!(fixed_zone(86_400).is_none());
        assert!(fixed_zone(-86_400).is_none());
    }

    #[test]
    fn days_count_and_step_on_the_calendar() {
        let day = |text: &str| parse_day(text).unwrap();
        assert_eq!(days_between(day("2026-10-09"), day("2026-10-12")), 3);
        assert_eq!(days_between(day("2026-10-12"), day("2026-10-09")), -3);
        assert_eq!(
            days_before(day("2026-03-01"), 1).map(day_text).as_deref(),
            Some("2026-02-28")
        );
        assert_eq!(
            days_before(day("2026-10-12"), -3).map(day_text).as_deref(),
            Some("2026-10-15")
        );
        assert_eq!(days_before(day("2026-10-12"), 4_000_000_000), None);
        assert_eq!(epoch_secs(day("1970-01-02"), 0, 0, 1), Some(86_401));
        assert_eq!(epoch_secs(day("2026-10-12"), 24, 0, 0), None);
        assert_eq!(org_stamp(day("2026-10-12"), 9, 5), "[2026-10-12 Mon 09:05]");
    }

    /// The zone a process reads from `TZ`, read the way a process reads it.
    #[test]
    fn the_local_zone_follows_tz_and_refreshes_after_a_second() {
        fn render() -> String {
            let wall = wall_in(at("2026-10-12T14:00:00Z"), &local_zone()).unwrap();
            format!("{} {}", wall.second(), wall.offset())
        }
        fn on(tz: &str) -> String {
            unsafe { std::env::set_var("TZ", tz) };
            std::thread::spawn(render).join().unwrap()
        }
        let _lock = crate::test_env_lock();
        assert_eq!(on("IST-5:30"), "2026-10-12 19:30:00 +05:30");
        assert_eq!(on("EST5EDT,M3.2.0,M11.1.0"), "2026-10-12 10:00:00 -04:00");
        assert_eq!(on(""), "2026-10-12 14:00:00 +00:00");
        unsafe { std::env::set_var("TZ", "IST-5:30") };
        let same_thread = std::thread::spawn(|| {
            let before = render();
            unsafe { std::env::set_var("TZ", "EST5EDT,M3.2.0,M11.1.0") };
            std::thread::sleep(std::time::Duration::from_millis(1100));
            (before, render())
        })
        .join()
        .unwrap();
        assert_eq!(same_thread.0, "2026-10-12 19:30:00 +05:30");
        assert_eq!(same_thread.1, "2026-10-12 10:00:00 -04:00");
        unsafe { std::env::remove_var("TZ") };
    }
}
