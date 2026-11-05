//! The set-aside's times: a pass's folder name and an entry's time, in UTC.

use std::time::{SystemTime, UNIX_EPOCH};

/// `now` in UTC as `YYYYMMDDTHHMMSS-mmmZ`: a pass's folder name.
pub(super) fn timestamp(now: SystemTime) -> String {
    let (year, month, day, secs, millis) = utc(now);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}-{millis:03}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
    )
}

/// `now` in UTC as RFC 3339 to the millisecond: `YYYY-MM-DDTHH:MM:SS.mmmZ`.
pub(super) fn rfc3339(now: SystemTime) -> String {
    let (year, month, day, secs, millis) = utc(now);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}.{millis:03}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60,
    )
}

/// `now`'s UTC date, its seconds into the day and its milliseconds; the epoch for a time
/// before it.
fn utc(now: SystemTime) -> (u64, u64, u64, u64, u32) {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (year, month, day) = civil(secs / 86_400);
    (year, month, day, secs % 86_400, since.subsec_millis())
}

/// The proleptic Gregorian date `days` after 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day)
}
