//! A matter the lawyer keeps on a network share: a path an agent names inside it is the
//! lawyer's own share, not a server the agent picked, so it is not refused for being on the
//! network (the request goes to the lawyer as before). Decided from the two texts alone and
//! strictly: one spelling of the matter folder, no `.` or `..` anywhere, names compared with
//! ASCII case folding only, so any doubt leaves the path a network path an agent named.

use std::path::Path;

use super::NetEnv;
use super::resolve::{Disk, Walked, walk};

/// Whether a path an agent named (absolute, or taken from the matter folder `matter`) may be
/// handed to the file system: only when the network-safe walk vouches for it (no network
/// location in its text, on its way or as a link's target, nothing it can't look at:
/// [`walk`] on `disk`, fail-closed). Never one that lies by its text in `matter` kept on a
/// share: that one is the lawyer's own, judged by its text alone, untouched.
pub(crate) fn may_touch(path: &Path, matter: &Path, disk: &Disk<'_>) -> bool {
    let joined = matter.join(path);
    let net = disk.net;
    if under_share_matter(path, matter, net) || under_share_matter(&joined, matter, net) {
        return false;
    }
    !net.is_network(path) && matches!(walk(&joined, disk), Walked::Local(_))
}

/// Whether `path` lies, by its text alone, at or under `matter` when `matter` is itself a
/// network location ([`NetEnv::is_network`]). Never touches the disk.
///
/// Strict: separators normalised (`/` read as `\` with Windows' rules); any `.` or `..` name,
/// an empty name past the matter's (a doubled separator) or, with Windows' rules, a name the
/// system would trim (a trailing dot or space) is never under; the matter's names must open
/// the path's in the same form (a `Z:\` matter never matches `\\server\share\…`, nor
/// `\\?\UNC\server\…` a `\\server\…` one); names compare ordinally, ignoring ASCII case alone
/// when `net.fold_case` (the Kelvin sign is not `k`).
pub(crate) fn under_share_matter(path: &Path, matter: &Path, net: &NetEnv) -> bool {
    if !net.is_network(matter) {
        return false;
    }
    let (Some(path), Some(matter)) = (path.to_str(), matter.to_str()) else {
        return false;
    };
    let (path, matter) = (names(path, net.windows), names(matter, net.windows));
    let dotted = |name: &&str| *name == "." || *name == "..";
    if matter.is_empty() || matter.iter().any(dotted) || path.iter().any(dotted) {
        return false;
    }
    if path.len() < matter.len() {
        return false;
    }
    let (head, tail) = path.split_at(matter.len());
    let same = |(got, want): (&&str, &&str)| {
        got == want || (net.fold_case && got.eq_ignore_ascii_case(want))
    };
    head.iter().zip(&matter).all(same)
        && tail.iter().all(|name| {
            !name.is_empty() && !(net.windows && (name.ends_with('.') || name.ends_with(' ')))
        })
}

/// `text`'s names split at its separators (`\` and `/` with Windows' rules, `/` otherwise),
/// the empty names a leading `\\` or `/` makes kept, one trailing separator dropped.
fn names(text: &str, windows: bool) -> Vec<&str> {
    let separator = |c: char| c == '/' || (windows && c == '\\');
    let text = text.strip_suffix(separator).unwrap_or(text);
    text.split(separator).collect()
}
