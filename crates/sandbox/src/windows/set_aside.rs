//! Protected names the agent made, moved aside before the next launch (S15 PR3). NTFS holds no
//! entry for a missing name, so a protected name ([`PROTECTED_NAMES`], and `.git`) that doesn't
//! exist at launch gets no deny, and the agent can make it. Before the next launch the desktop's
//! factory calls [`set_aside_protected`]: each such name at the matter's root that the matter's
//! pool user owns (it made it) moves to `<state>\set-aside\<timestamp>\`, so the lawyer's own
//! tools never load it. Names the lawyer made stay, and get the deny.
//!
//! Known limit: a nested `CLAUDE.md` the agent makes below the root is not moved here.

use std::io;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use super::admit::PathKind;
#[cfg(windows)]
use super::sids::Sid;
use super::write;
use super::writer::ApplyFailure;
use crate::{Error, PROTECTED_NAMES, Policy, SetAside};

/// Also moved: hooks in a `.git` the agent made would run in the lawyer's own git.
const ALSO: [&str; 1] = [".git"];

/// Moves each protected name at the matter's root whose owner is `pool_user` (read without
/// following a link) to `<state>\set-aside\<timestamp>\`, `now` giving the timestamp.
/// Returns what moved, for R17's notice. Fails with [`Error::AclApply`] naming
/// [`PathKind::ReadOnlyInMatter`] and the kind of error.
#[cfg(windows)]
pub fn set_aside_protected(
    policy: &Policy,
    pool_user: &Sid,
    now: SystemTime,
) -> Result<Vec<SetAside>, Error> {
    set_aside_with(
        policy,
        |path| Ok(super::acl::owner(path)? == *pool_user),
        now,
    )
}

/// [`set_aside_protected`] with the owner check `owned` as a parameter.
pub fn set_aside_with(
    policy: &Policy,
    mut owned: impl FnMut(&Path) -> io::Result<bool>,
    now: SystemTime,
) -> Result<Vec<SetAside>, Error> {
    let folder = policy.state().join("set-aside").join(timestamp(now));
    let mut moved = Vec::new();
    for name in PROTECTED_NAMES.iter().chain(&ALSO) {
        let from = policy.matter().join(name);
        match std::fs::symlink_metadata(&from) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => return Err(failed(&error)),
        }
        if !owned(&from).map_err(|e| failed(&e))? {
            continue;
        }
        let to = folder.join(name);
        write::move_aside(&from, &to).map_err(|e| failed(&e))?;
        moved.push(SetAside { from, to });
    }
    if !moved.is_empty() {
        tracing::info!(
            count = moved.len(),
            "protected names the agent made were set aside"
        );
    }
    Ok(moved)
}

/// `now` in UTC as `YYYYMMDDTHHMMSS-mmmZ`.
fn timestamp(now: SystemTime) -> String {
    let since = now.duration_since(UNIX_EPOCH).unwrap_or_default();
    let secs = since.as_secs();
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (year, month, day) = civil(days);
    format!(
        "{year:04}{month:02}{day:02}T{:02}{:02}{:02}-{:03}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60,
        since.subsec_millis()
    )
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

fn failed(error: &io::Error) -> Error {
    Error::AclApply {
        path_kind: PathKind::ReadOnlyInMatter,
        failure: ApplyFailure::Io(error.kind()),
    }
}

#[cfg(test)]
mod tests;
