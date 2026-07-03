//! Parses the ISO8601 timestamps found on transcript lines.
//!
//! Ports `Parsing/TranscriptTimestampParser.swift`. Both Claude and Codex
//! transcripts stamp lines like `2026-06-12T05:07:51.103Z`; some lines omit
//! fractional seconds. The Swift original leans on `Date.ISO8601FormatStyle`;
//! we hand-roll the parse so no datetime crate is pulled into the workspace.
//!
//! DIVERGENCE: Foundation's `Date` is represented here as
//! [`Timestamp`], an epoch-millisecond integer. Millisecond precision is
//! exact for every fractional-second form the transcripts emit (three
//! digits), and integer equality avoids the float-comparison fragility a
//! `f64`-seconds representation would carry into the parser's equality
//! assertions.

use serde::{Deserialize, Serialize};

/// A wall-clock instant, stored as milliseconds since the Unix epoch.
///
/// Stands in for Foundation's `Date`. `Timestamp::EPOCH_ZERO` is the
/// `Date(timeIntervalSince1970: 0)` fallback the Swift parsers reach for when
/// a line carries no timestamp and none has been seen yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp {
    /// Milliseconds since 1970-01-01T00:00:00Z.
    pub millis: i64,
}

impl Timestamp {
    /// The `Date(timeIntervalSince1970: 0)` fallback instant.
    pub const EPOCH_ZERO: Timestamp = Timestamp { millis: 0 };

    /// Builds a timestamp from epoch milliseconds.
    pub const fn from_millis(millis: i64) -> Self {
        Timestamp { millis }
    }
}

/// Parses the ISO8601 timestamps found on transcript lines.
#[derive(Debug, Default, Clone, Copy)]
pub struct TranscriptTimestampParser;

impl TranscriptTimestampParser {
    /// Creates a timestamp parser.
    pub fn new() -> Self {
        TranscriptTimestampParser
    }

    /// Parses an ISO8601 timestamp string.
    ///
    /// Returns `None` when the input is absent or malformed, mirroring the
    /// Swift `date(from:)` fail-open contract.
    pub fn date(&self, raw: Option<&str>) -> Option<Timestamp> {
        raw.and_then(parse_iso8601).map(Timestamp::from_millis)
    }
}

/// Number of days from the Unix epoch to the civil date `y-m-d`.
///
/// Howard Hinnant's `days_from_civil`, valid for the proleptic Gregorian
/// calendar over the full range the transcripts can carry.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// Parses a subset of ISO8601 wide enough for the transcript formats:
/// `YYYY-MM-DDTHH:MM:SS`, an optional `.fff` fractional part, and a zone that
/// is `Z`, `±HH:MM`, or `±HHMM` (absent zone treated as UTC).
///
/// Returns epoch milliseconds, or `None` on any shape mismatch.
fn parse_iso8601(raw: &str) -> Option<i64> {
    let raw = raw.trim();
    let (date_part, rest) = raw.split_once('T').or_else(|| raw.split_once('t'))?;

    // Date: YYYY-MM-DD.
    let mut date_iter = date_part.split('-');
    let year: i64 = date_iter.next()?.parse().ok()?;
    let month: i64 = date_iter.next()?.parse().ok()?;
    let day: i64 = date_iter.next()?.parse().ok()?;
    if date_iter.next().is_some() || !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }

    // Split off the zone designator from the tail.
    let (time_core, offset_seconds) = split_zone(rest)?;

    // Time core: HH:MM:SS with an optional .fff fractional part.
    let (hms, frac) = match time_core.split_once('.') {
        Some((hms, frac)) => (hms, Some(frac)),
        None => (time_core, None),
    };
    let mut time_iter = hms.split(':');
    let hour: i64 = time_iter.next()?.parse().ok()?;
    let minute: i64 = time_iter.next()?.parse().ok()?;
    let second: i64 = time_iter.next()?.parse().ok()?;
    if time_iter.next().is_some()
        || !(0..=23).contains(&hour)
        || !(0..=59).contains(&minute)
        || !(0..=60).contains(&second)
    {
        return None;
    }

    let frac_ms = fractional_millis(frac)?;

    let days = days_from_civil(year, month, day);
    let base_seconds = days * 86_400 + hour * 3_600 + minute * 60 + second;
    let millis = base_seconds * 1_000 + frac_ms - offset_seconds * 1_000;
    Some(millis)
}

/// Splits the trailing zone designator off a time string, returning the
/// zone-free time core and the zone's offset in seconds (positive east of
/// UTC). A missing zone is treated as UTC (`0`).
fn split_zone(time: &str) -> Option<(&str, i64)> {
    if let Some(core) = time.strip_suffix('Z').or_else(|| time.strip_suffix('z')) {
        return Some((core, 0));
    }
    // A '+'/'-' after the seconds marks an offset; the fractional part never
    // carries a sign, so scanning from a fixed minimum index is safe.
    let bytes = time.as_bytes();
    for (idx, &b) in bytes.iter().enumerate().skip(1) {
        if b == b'+' || b == b'-' {
            let sign = if b == b'+' { 1 } else { -1 };
            let zone = &time[idx + 1..];
            let offset = parse_zone_offset(zone)?;
            return Some((&time[..idx], sign * offset));
        }
    }
    Some((time, 0))
}

/// Parses a `HH:MM` / `HHMM` / `HH` zone offset into seconds.
fn parse_zone_offset(zone: &str) -> Option<i64> {
    let digits: String = zone.chars().filter(|c| c.is_ascii_digit()).collect();
    let (hh, mm) = match digits.len() {
        2 => (&digits[..2], "0"),
        4 => (&digits[..2], &digits[2..4]),
        _ => return None,
    };
    let hours: i64 = hh.parse().ok()?;
    let minutes: i64 = mm.parse().ok()?;
    Some(hours * 3_600 + minutes * 60)
}

/// Converts a fractional-second digit string to milliseconds, taking the
/// first three digits (right-padded). `None` for a non-digit fractional part.
fn fractional_millis(frac: Option<&str>) -> Option<i64> {
    let Some(frac) = frac else { return Some(0) };
    if frac.is_empty() || !frac.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let mut ms = 0i64;
    let mut taken = 0;
    for b in frac.bytes() {
        if taken >= 3 {
            break;
        }
        ms = ms * 10 + i64::from(b - b'0');
        taken += 1;
    }
    while taken < 3 {
        ms *= 10;
        taken += 1;
    }
    Some(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fractional_seconds() {
        let parser = TranscriptTimestampParser::new();
        let ts = parser.date(Some("2026-06-12T05:07:51.103Z")).unwrap();
        // 2026-06-12T05:07:51Z base plus 103 ms.
        let expected = parse_iso8601("2026-06-12T05:07:51Z").unwrap() + 103;
        assert_eq!(ts.millis, expected);
    }

    #[test]
    fn plain_and_fractional_agree_on_the_same_instant() {
        let parser = TranscriptTimestampParser::new();
        let with_zeros = parser.date(Some("2026-06-12T10:00:00.000Z")).unwrap();
        let without = parser.date(Some("2026-06-12T10:00:00Z")).unwrap();
        assert_eq!(with_zeros, without);
    }

    #[test]
    fn honors_zone_offsets() {
        // 12:00 at +02:00 is 10:00 UTC.
        let east = parse_iso8601("2026-06-12T12:00:00+02:00").unwrap();
        let utc = parse_iso8601("2026-06-12T10:00:00Z").unwrap();
        assert_eq!(east, utc);
    }

    #[test]
    fn absent_or_malformed_is_none() {
        let parser = TranscriptTimestampParser::new();
        assert_eq!(parser.date(None), None);
        assert_eq!(parser.date(Some("not a date")), None);
        assert_eq!(parser.date(Some("2026-13-40T99:99:99Z")), None);
    }

    #[test]
    fn epoch_zero_is_millis_zero() {
        assert_eq!(Timestamp::EPOCH_ZERO.millis, 0);
        assert_eq!(parse_iso8601("1970-01-01T00:00:00Z"), Some(0));
    }
}
