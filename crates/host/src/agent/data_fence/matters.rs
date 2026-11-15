//! The host's auto-deny for other matters (`agent::matter_fence`): a permission request whose
//! paths, search folders or shell words reach another matter's fence paths is refused unasked,
//! as one that reaches `<data>` is ([`FenceCheck`]), and named by that matter (the innermost,
//! when matters nest) so the lawyer can be offered the invite. Every other matter counts, an
//! invited one too: an invite is read-only, and its reads need no request (Claude's `Read`
//! allow, no deny for Codex).

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_protocol::MatterId;

use super::resolve::{self, Disk, Found};
use super::{FenceCheck, key, variables};

/// Each other matter and its fence paths, as the session's launch made them.
pub(crate) type Fenced = Vec<(MatterId, Vec<PathBuf>)>;

/// The other matter of `matters` the request `check` reaches, if any: a path that leads into
/// one of its fence paths, a search folder that is in one or holds one, or a shell word that
/// names one by its text up to a name's end. Compared ignoring case when `insensitive`.
/// Stats the file system through `disk`.
pub(crate) fn other_matter_on(
    check: &FenceCheck,
    matters: &[(MatterId, Vec<PathBuf>)],
    insensitive: bool,
    disk: &Disk<'_>,
) -> Option<MatterId> {
    let fenced: Vec<(MatterId, Vec<OsString>)> = matters
        .iter()
        .flat_map(|(id, paths)| paths.iter().map(|path| (*id, key(path, insensitive))))
        .collect();
    if fenced.is_empty() {
        return None;
    }
    let innermost = |path: &Path| {
        let at = key(path, insensitive);
        fenced
            .iter()
            .filter(|(_, place)| at.starts_with(place))
            .max_by_key(|(_, place)| place.len())
            .map(|(id, _)| *id)
    };
    let holds = |root: &Path| {
        let at = key(root, insensitive);
        fenced
            .iter()
            .find(|(_, place)| place.starts_with(&at))
            .map(|(id, _)| *id)
    };
    let var = |name: &str| {
        let launch = check
            .vars
            .iter()
            .find(|(var, _)| var.eq_ignore_ascii_case(name));
        launch
            .map(|(_, value)| value.clone())
            .or_else(|| std::env::var(name).ok())
    };
    let home = check.home.as_deref();
    let found = |path: &PathBuf, root: bool| -> Vec<PathBuf> {
        let paths = variables::judged(path, disk.net.windows, root, &var).unwrap_or_default();
        paths
            .iter()
            .flat_map(|path| resolve::resolve_all(path, &check.cwd, home, disk))
            .filter_map(|found| match found {
                Found::Path(path) => Some(path),
                Found::Refused | Found::ShareMatter => None,
            })
            .collect()
    };
    let by_path = check
        .paths
        .iter()
        .flat_map(|path| found(path, false))
        .find_map(|path| innermost(&path));
    let by_root = || {
        check
            .roots
            .iter()
            .flat_map(|root| found(root, true))
            .find_map(|root| innermost(&root).or_else(|| holds(&root)))
    };
    by_path
        .or_else(by_root)
        .or_else(|| named_in_words(&check.words, matters, insensitive))
}

/// The innermost other matter one of `words` names by a fence path's text, `\` and `/` read
/// alike, followed by the word's end, a separator, a quote or a space: a matter `Smith` is not
/// named by `…\Smith 2\x`.
fn named_in_words(
    words: &[String],
    matters: &[(MatterId, Vec<PathBuf>)],
    insensitive: bool,
) -> Option<MatterId> {
    let norm = |text: &str| {
        let text = text.replace('\\', "/");
        if insensitive {
            text.to_lowercase()
        } else {
            text
        }
    };
    let mut best: Option<(usize, MatterId)> = None;
    for word in words.iter().map(|word| norm(word)) {
        for (id, paths) in matters {
            for path in paths.iter().filter_map(|path| path.to_str()) {
                let needle = norm(path);
                let needle = needle.trim_end_matches('/');
                if needle.is_empty() {
                    continue;
                }
                let named = word.match_indices(needle).any(|(at, _)| {
                    let rest = word.get(at + needle.len()..).unwrap_or_default();
                    rest.is_empty() || rest.starts_with(['/', '"', '\'', ' ', '\t'])
                });
                if named && best.is_none_or(|(len, _)| needle.len() > len) {
                    best = Some((needle.len(), *id));
                }
            }
        }
    }
    best.map(|(_, id)| id)
}

#[cfg(test)]
#[path = "matters_check_tests.rs"]
mod tests;
