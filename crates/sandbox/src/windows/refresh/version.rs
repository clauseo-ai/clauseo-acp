//! The versions the refresh compares (ADR 0049, "Admin copy refresh"):
//!
//! - the signature's `version`: the release's semver, as release.yml signs it (`X.Y.Z`, or
//!   `X.Y.Z-pre` for a prerelease);
//! - the staged exe's `FILEVERSION`: `build.rs` writes the same version's core as `X.Y.Z.0`;
//! - the record's `AdminCopyVersion`: the signed version of the copy in place, as signed.
//!
//! A version is read as semver, with a leading `v` dropped; a four-part `X.Y.Z.0` (the
//! `FILEVERSION` form, which a record may hold) reads as the release `X.Y.Z`, and a non-zero
//! fourth part is no version. Versions are ordered by semver precedence: a prerelease sorts
//! below its release, and build metadata doesn't count. Anything unreadable refuses the run.

use std::cmp::Ordering;

use semver::Version;

/// `text` as a version, as the module says; `None` when it isn't one.
pub fn parse(text: &str) -> Option<Version> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    if let Ok(version) = Version::parse(text) {
        return Some(version);
    }
    let parts: Vec<&str> = text.split('.').collect();
    let number = |part: &str| {
        let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
        digits.then(|| part.parse::<u64>().ok()).flatten()
    };
    match parts.as_slice() {
        [x, y, z, "0"] => Some(Version::new(number(x)?, number(y)?, number(z)?)),
        _ => None,
    }
}

/// How `candidate` orders against `recorded` by semver precedence; `None` when either can't be
/// read.
pub fn compare(recorded: &str, candidate: &str) -> Option<Ordering> {
    Some(parse(candidate)?.cmp_precedence(&parse(recorded)?))
}

/// Whether `candidate` is not older than `recorded` by semver precedence: `1.4.0-beta.1` is
/// older than `1.4.0`, `1.4.0` and `1.4.0.0` are the same. `false` when either can't be read.
pub fn version_not_older(recorded: &str, candidate: &str) -> bool {
    compare(recorded, candidate).is_some_and(Ordering::is_ge)
}

/// Whether the `FILEVERSION` `file` is the signed version `signed`: its first three parts are
/// the semver core, and its fourth is 0. A prerelease's `FILEVERSION` is its core's.
pub fn matches_file_version(signed: &str, file: [u16; 4]) -> bool {
    let Some(signed) = parse(signed) else {
        return false;
    };
    let [x, y, z, w] = file.map(u64::from);
    w == 0 && [signed.major, signed.minor, signed.patch] == [x, y, z]
}

#[cfg(test)]
mod tests;
