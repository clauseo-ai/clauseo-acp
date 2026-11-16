//! The version order `--adopt` and the refresh task (PR 3) compare admin copies by.

/// `text` as a version: semver, with a leading `v` dropped; a four-part `X.Y.Z.0` (the
/// `FILEVERSION` form, which a record may hold) reads as the release `X.Y.Z`, and a non-zero
/// fourth part is no version. `--adopt` records [`APP_VERSION`](super::APP_VERSION)
/// (`tauri.conf.json`'s `version`) and the refresh task (PR 3) the signed version, both read
/// this way.
pub fn parse_version(text: &str) -> Option<semver::Version> {
    let text = text.trim();
    let text = text.strip_prefix('v').unwrap_or(text);
    if let Ok(version) = semver::Version::parse(text) {
        return Some(version);
    }
    let parts: Vec<&str> = text.split('.').collect();
    let number = |part: &str| {
        let digits = !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
        digits.then(|| part.parse::<u64>().ok()).flatten()
    };
    match parts.as_slice() {
        [x, y, z, "0"] => Some(semver::Version::new(number(x)?, number(y)?, number(z)?)),
        _ => None,
    }
}

/// How `candidate` orders against `recorded` by semver precedence; `None` when either can't be
/// read ([`parse_version`]).
pub fn compare_versions(recorded: &str, candidate: &str) -> Option<std::cmp::Ordering> {
    Some(parse_version(candidate)?.cmp_precedence(&parse_version(recorded)?))
}

/// Whether `candidate` is no older than `recorded` by semver precedence: `--adopt` and the
/// refresh task (PR 3) never replace a copy with an older one. `1.4.0-beta.1` is older than
/// `1.4.0`, `1.4.0` and `1.4.0.0` are the same, and build metadata doesn't count. A version
/// that doesn't parse is never "not older": the safe side.
pub fn version_not_older(recorded: &str, candidate: &str) -> bool {
    compare_versions(recorded, candidate).is_some_and(std::cmp::Ordering::is_ge)
}
