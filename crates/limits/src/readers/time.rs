//! A strict RFC 3339 reader for the reset times in usage reads:
//! `YYYY-MM-DDTHH:MM:SS[.frac](Z|±HH:MM)`, to milliseconds since the epoch.

use clauseo_protocol::Timestamp;

/// The most fraction digits read; more is not a time this crate expects.
const MAX_FRACTION: usize = 9;

/// `text` as a [`Timestamp`], or `None` when it is not an RFC 3339 date-time: a bad shape, a
/// day the month does not have (leap years counted), an hour past 23, a minute or second past
/// 59, or an offset past 23:59. `T` and `Z` may be lower case. A fraction past milliseconds is
/// cut, not rounded.
pub(crate) fn parse_rfc3339(text: &str) -> Option<Timestamp> {
    let bytes = text.as_bytes();
    let year = digits(bytes, 0, 4)?;
    let month = digits(bytes, 5, 2)?;
    let day = digits(bytes, 8, 2)?;
    let hour = digits(bytes, 11, 2)?;
    let minute = digits(bytes, 14, 2)?;
    let second = digits(bytes, 17, 2)?;
    let separators = [(4, b'-'), (7, b'-'), (13, b':'), (16, b':')];
    if separators
        .iter()
        .any(|&(at, byte)| bytes.get(at) != Some(&byte))
    {
        return None;
    }
    if !matches!(bytes.get(10), Some(b'T' | b't')) {
        return None;
    }
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let mut at = 19;
    let mut millis = 0;
    if bytes.get(at) == Some(&b'.') {
        let start = at + 1;
        let end = bytes
            .get(start..)?
            .iter()
            .position(|byte| !byte.is_ascii_digit())
            .map_or(bytes.len(), |n| start + n);
        let count = end - start;
        if count == 0 || count > MAX_FRACTION {
            return None;
        }
        millis = bytes
            .get(start..end)?
            .iter()
            .take(3)
            .enumerate()
            .fold(0, |sum, (i, byte)| {
                sum + i64::from(byte - b'0') * [100, 10, 1].get(i).copied().unwrap_or(0)
            });
        at = end;
    }
    let offset_mins = offset(bytes.get(at..)?)?;
    let days = days_from_civil(year, month, day);
    let seconds = days * 86_400 + hour * 3_600 + minute * 60 + second - offset_mins * 60;
    Timestamp::new(seconds.checked_mul(1000)?.checked_add(millis)?).ok()
}

/// The offset in minutes east of UTC: `Z`, or `+HH:MM` / `-HH:MM`, and nothing after it.
fn offset(bytes: &[u8]) -> Option<i64> {
    match bytes {
        [b'Z' | b'z'] => Some(0),
        [sign @ (b'+' | b'-'), ..] if bytes.len() == 6 && bytes.get(3) == Some(&b':') => {
            let hours = digits(bytes, 1, 2)?;
            let minutes = digits(bytes, 4, 2)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let mins = hours * 60 + minutes;
            Some(if *sign == b'-' { -mins } else { mins })
        }
        _ => None,
    }
}

/// The number written by `len` ASCII digits at `at`.
fn digits(bytes: &[u8], at: usize, len: usize) -> Option<i64> {
    bytes
        .get(at..at.checked_add(len)?)?
        .iter()
        .try_fold(0, |sum, &byte| {
            byte.is_ascii_digit()
                .then(|| sum * 10 + i64::from(byte - b'0'))
        })
}

/// The days of `month` in `year`, leap years counted.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to the date, in the proleptic Gregorian calendar (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let year_of_era = year - era * 400;
    let month_from_march = (month + 9) % 12;
    let day_of_year = (153 * month_from_march + 2) / 5 + day - 1;
    let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
    era * 146_097 + day_of_era - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(text: &str) -> Option<i64> {
        parse_rfc3339(text).map(Timestamp::as_millis)
    }

    #[test]
    fn utc_times_read_to_milliseconds() {
        assert_eq!(ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(ms("2026-11-02T14:30:00Z"), Some(1_793_629_800_000));
        assert_eq!(ms("2026-11-02t14:30:00z"), Some(1_793_629_800_000));
        assert_eq!(ms("1969-12-31T23:59:59Z"), Some(-1000));
    }

    #[test]
    fn fractions_are_cut_to_milliseconds() {
        assert_eq!(ms("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(ms("1970-01-01T00:00:01.25Z"), Some(1250));
        assert_eq!(ms("1970-01-01T00:00:01.123999999Z"), Some(1123));
        assert_eq!(ms("1970-01-01T00:00:01.Z"), None);
        assert_eq!(ms("1970-01-01T00:00:01.1234567890Z"), None);
    }

    #[test]
    fn offsets_move_the_time_to_utc() {
        assert_eq!(ms("2026-11-02T15:30:00+01:00"), Some(1_793_629_800_000));
        assert_eq!(ms("2026-11-02T09:00:00-05:30"), Some(1_793_629_800_000));
        assert_eq!(ms("2026-11-02T14:30:00.250+00:00"), Some(1_793_629_800_250));
        assert_eq!(ms("2026-11-02T14:30:00+24:00"), None);
        assert_eq!(ms("2026-11-02T14:30:00+01:60"), None);
        assert_eq!(ms("2026-11-02T14:30:00+0100"), None);
    }

    #[test]
    fn days_are_checked_against_the_month_and_leap_years() {
        assert!(ms("2024-02-29T00:00:00Z").is_some());
        assert!(ms("2000-02-29T00:00:00Z").is_some());
        assert_eq!(ms("2026-02-29T00:00:00Z"), None);
        assert_eq!(ms("1900-02-29T00:00:00Z"), None);
        assert_eq!(ms("2026-04-31T00:00:00Z"), None);
        assert!(ms("2026-12-31T00:00:00Z").is_some());
        assert_eq!(ms("2026-13-01T00:00:00Z"), None);
        assert_eq!(ms("2026-00-01T00:00:00Z"), None);
        assert_eq!(ms("2026-01-00T00:00:00Z"), None);
    }

    #[test]
    fn bad_shapes_and_fields_are_none() {
        for text in [
            "",
            "2026-11-02",
            "2026-11-02T14:30Z",
            "2026-11-02 14:30:00Z",
            "2026-11-02T14:30:00",
            "2026-11-02T14:30:00Zjunk",
            "2026-11-02T24:00:00Z",
            "2026-11-02T23:60:00Z",
            "2026-11-02T23:59:60Z",
            "+2026-11-02T14:30:00Z",
            "2026-1-02T14:30:00Z",
            "2026-11-02T14:30:00+01",
            "２０２６-11-02T14:30:00Z",
        ] {
            assert_eq!(ms(text), None, "{text:?}");
        }
    }
}
