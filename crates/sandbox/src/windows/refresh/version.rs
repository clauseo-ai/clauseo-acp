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

pub use crate::windows::admin_copy::{
    compare_versions as compare, parse_version as parse, version_not_older,
};

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
