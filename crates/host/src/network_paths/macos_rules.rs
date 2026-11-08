//! macOS's ways to reach a mount by another text, decided from the text alone: `/.nofollow/…`,
//! `/.resolve/…` and `/.vol/…` (which reach an SMB mount without its mount point's text),
//! `/home/…` (the `auto_home` trigger, which mounts a home from a directory server on an AD-bound
//! Mac), and the data volume's own spelling of a firmlinked folder (`/System/Volumes/Data/Users`
//! is `/Users`), put in its firmlinked form before the mount list is compared. Pure, so it is
//! tested on every system.

use std::ffi::OsStr;

use super::{NetEnv, fold_name, name_folds_to};

/// The first names that reach a mount by another text.
const OTHER_TEXT: [&str; 3] = [".nofollow", ".resolve", ".vol"];

/// The data volume's mount point.
const DATA: [&str; 3] = ["system", "volumes", "data"];

/// What macOS's rules make of a path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Mac {
    /// Refused: a first name in [`OTHER_TEXT`], `/home`, the data volume's `home`, or a data
    /// volume path when the firmlinks are unknown.
    Refused,
    /// A data volume path in its firmlinked form.
    Rewritten(String),
    /// To be checked as it is.
    AsIs,
}

/// `text` under macOS's rules ([`Mac`]). Names fold as a case-insensitive volume does
/// ([`name_folds_to`]).
pub(super) fn firmlinked(text: &str, net: &NetEnv) -> Mac {
    let Some(rest) = text.strip_prefix('/') else {
        return Mac::AsIs;
    };
    let names: Vec<&str> = rest.split('/').filter(|name| !name.is_empty()).collect();
    let is = |name: Option<&&str>, ascii: &str| {
        name.is_some_and(|name| name_folds_to(OsStr::new(name), ascii))
    };
    if OTHER_TEXT.iter().any(|other| is(names.first(), other)) || is(names.first(), "home") {
        return Mac::Refused;
    }
    let on_data = DATA
        .iter()
        .enumerate()
        .all(|(at, want)| is(names.get(at), want));
    if !on_data {
        return Mac::AsIs;
    }
    let Some(firmlinks) = net.firmlinks.as_ref() else {
        return Mac::Refused;
    };
    let inside = names.get(DATA.len()..).unwrap_or_default();
    if is(inside.first(), "home") {
        return Mac::Refused;
    }
    for (root, place) in firmlinks {
        let place: Vec<&str> = place.split('/').filter(|name| !name.is_empty()).collect();
        let same = place.len() <= inside.len()
            && place
                .iter()
                .zip(inside)
                .all(|(want, got)| fold_name(want) == fold_name(got));
        if same && !place.is_empty() {
            let after = inside.get(place.len()..).unwrap_or_default();
            let mut form = root.trim_end_matches('/').to_owned();
            for name in after {
                form.push('/');
                form.push_str(name);
            }
            return Mac::Rewritten(form);
        }
    }
    Mac::AsIs
}

/// `/usr/share/firmlinks`' lines: each a root folder, a tab, and its place on the data volume.
#[cfg(any(target_os = "macos", test))]
pub(super) fn parse_firmlinks(text: &str) -> Vec<(String, String)> {
    text.lines()
        .filter_map(|line| {
            let (root, place) = line.split_once('\t')?;
            let (root, place) = (root.trim(), place.trim());
            (root.starts_with('/') && !place.is_empty())
                .then(|| (root.to_owned(), place.to_owned()))
        })
        .collect()
}
