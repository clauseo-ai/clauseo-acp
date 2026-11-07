//! The version order `--adopt` and the refresh task (PR 3) compare admin copies by.

/// Whether `candidate` is no older than `recorded`, both versions as `tauri.conf.json` writes
/// them (`1.2.3`, `1.2.3-beta.1`; build metadata after `+` ignored): `--adopt` and the refresh
/// task (PR 3) never replace a copy with an older one. The numeric parts compare first, up to
/// four (a missing one is 0); with equal parts a release beats a pre-release, and two
/// pre-releases compare part by part as semver does (numbers below words). A version that
/// doesn't parse is never "not older": the safe side.
pub fn version_not_older(recorded: &str, candidate: &str) -> bool {
    match (Version::parse(recorded), Version::parse(candidate)) {
        (Some(recorded), Some(candidate)) => candidate >= recorded,
        _ => false,
    }
}

/// A version as [`version_not_older`] compares it.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Version {
    core: [u64; 4],
    /// `true` for a release, so it sorts above any pre-release of the same core.
    release: bool,
    pre: Vec<PreId>,
}

/// One dot-separated pre-release identifier; numbers sort below words.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum PreId {
    Number(u64),
    Word(String),
}

impl Version {
    fn parse(text: &str) -> Option<Version> {
        let text = text.split('+').next().unwrap_or_default();
        let (core_text, pre_text) = match text.split_once('-') {
            Some((core, pre)) => (core, Some(pre)),
            None => (text, None),
        };
        let parts: Vec<u64> = core_text
            .split('.')
            .map(|part| {
                part.bytes()
                    .all(|b| b.is_ascii_digit())
                    .then(|| part.parse().ok())
                    .flatten()
            })
            .collect::<Option<_>>()?;
        if parts.is_empty() || parts.len() > 4 {
            return None;
        }
        let mut core = [0; 4];
        for (slot, part) in core.iter_mut().zip(parts) {
            *slot = part;
        }
        let pre = match pre_text {
            Some(pre) => pre
                .split('.')
                .map(|id| match id.parse() {
                    _ if id.is_empty() => None,
                    Ok(n) if id.bytes().all(|b| b.is_ascii_digit()) => Some(PreId::Number(n)),
                    _ => Some(PreId::Word(id.to_owned())),
                })
                .collect::<Option<Vec<_>>>()?,
            None => Vec::new(),
        };
        Some(Version {
            core,
            release: pre_text.is_none(),
            pre,
        })
    }
}
