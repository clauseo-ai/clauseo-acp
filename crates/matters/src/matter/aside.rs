//! Setting a marker aside: a `.clauseo/matter.json` an import cannot keep (one that does not
//! read, or one the host will not adopt) is renamed inside `.clauseo`, never deleted, so the
//! import can write its own. It stays in `.clauseo`, which every checkpoint and the paper list
//! exclude, so it is never versioned or shown.

use std::path::{Path, PathBuf};

use clauseo_protocol::Timestamp;

use super::{exists, json};
use crate::{Error, fsync, write};

/// How many `-<n>` suffixes are tried before giving up: one set-aside per import, so more than
/// this many in one second means something else is writing there.
const TRIES: u32 = 1000;

/// Renames `folder`'s `.clauseo/matter.json` to `.clauseo/matter.old-<stamp>.json`, `<stamp>`
/// being `at` in UTC as `20261111T153000Z`, or with `-2`, `-3`... before `.json` when that name
/// is taken; returns the new path. A missing marker is an [`Error::Io`] of kind `NotFound`.
pub fn set_aside_marker(folder: &Path, at: Timestamp) -> Result<PathBuf, Error> {
    let dir = folder.join(json::DIR);
    let marker = json::path(folder);
    let stamp = stamp(at);
    for n in 1..=TRIES {
        let name = if n == 1 {
            format!("matter.old-{stamp}.json")
        } else {
            format!("matter.old-{stamp}-{n}.json")
        };
        let target = dir.join(name);
        if exists(&target)? {
            continue;
        }
        write::rename(&marker, &target).map_err(|err| Error::io("rename", &marker, err))?;
        fsync::sync_dir(&dir).map_err(|err| Error::io("sync", &dir, err))?;
        return Ok(target);
    }
    Err(Error::history(
        "matter.json",
        "every name for the old marker is taken",
    ))
}

/// `at` in UTC as `YYYYMMDDTHHMMSSZ`.
pub(crate) fn stamp(at: Timestamp) -> String {
    let secs = at.as_millis().div_euclid(1000);
    let (days, time) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    let (year, month, day) = civil(days);
    let (hour, minute, second) = (time / 3600, time % 3600 / 60, time % 60);
    format!("{year:04}{month:02}{day:02}T{hour:02}{minute:02}{second:02}Z")
}

/// The proleptic Gregorian date `days` after 1970-01-01, as year, month and day (Howard
/// Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(ms: i64) -> Timestamp {
        Timestamp::new(ms).unwrap()
    }

    #[test]
    fn the_stamp_is_utc_to_the_second() {
        assert_eq!(stamp(ms(0)), "19700101T000000Z");
        // 2026-11-11 15:30:00.999 UTC.
        assert_eq!(stamp(ms(1_794_411_000_999)), "20261111T153000Z");
        // A leap day, and the last second of a year.
        assert_eq!(stamp(ms(951_782_400_000)), "20000229T000000Z");
        assert_eq!(stamp(ms(1_704_067_199_000)), "20231231T235959Z");
    }
}
