//! The fence between matters: what of every other matter the host knows (registered or
//! forgotten) a session's agent is kept out of, and what a per-conversation read invite opens
//! (the header's `read_matters`, `MatterReadAllowed`). Made at the launch: the launch paths
//! name the other matters ([`MatterFence::asked`]); the launch prep resolves their folders and
//! lists what nesting needs ([`MatterFence::resolve`]).
//!
//! For each other matter its fence paths ([`Other::paths`]) are its folder whole, each spelling
//! (where it leads, resolved like Undo's paths through `network_paths::touchable`, then as stored
//! when that differs; a folder on a share as stored alone, never touched), unless the folder
//! holds a kept place: the session's matter folder or `<data>` (each spelling), or, for a matter
//! the session is not invited into, a matter it is. Then they are the siblings along the way
//! down to each kept place, listed now (as `codex_fence::down_to` does for `<data>`), so a deny
//! never covers the session's own matter. A matter inside the session's matter is fenced whole;
//! the session's own matter never is. A folder on the way that can't be listed (or a matter on
//! a share that holds a kept place) leaves that spelling unfenced, with a warning: a deny of
//! the whole folder would cover the session's own.
//!
//! Who uses which: Claude's `Read` deny rules and Codex's profile name the fence paths of the
//! matters not invited, Claude's `Edit` deny rules and its Windows shell rules those of every
//! other matter (an invite is read-only, and on Windows Claude reads an invited matter through
//! its file tools only), and Claude's `Read` allow rules the folders of the invited ones
//! (`data_fence::claude_deny_rules`, `claude_allow_rules`, `codex_fence::rules`,
//! `data_fence::shell_deny::matters`). The residual, as for `<data>`: what is made in a parent
//! matter after the launch is in no rule until the next launch.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};

use clauseo_protocol::MatterId;

use crate::network_paths::resolve::Disk;
use crate::network_paths::{FOLD_CASE, NetEnv, fold_name, touchable};

/// Another matter, as a launch fences it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Other {
    /// Its id.
    pub(crate) id: MatterId,
    /// Its name at the launch, for the lawyer's line when its shell rules did not fit.
    pub(crate) name: String,
    /// Its folder as stored.
    pub(crate) folder: PathBuf,
    /// Whether the lawyer let this conversation read its files.
    pub(crate) invited: bool,
    /// Its folder's spellings ([`MatterFence::resolve`]); empty before.
    pub(crate) spellings: Vec<PathBuf>,
    /// What of it the session is kept out of (the module doc); empty before the resolve.
    pub(crate) paths: Vec<PathBuf>,
}

impl Other {
    /// `id`'s matter at `folder`, not yet resolved.
    pub(crate) fn new(id: MatterId, name: String, folder: PathBuf, invited: bool) -> Self {
        Self {
            id,
            name,
            folder,
            invited,
            spellings: Vec::new(),
            paths: Vec::new(),
        }
    }
}

/// What a launch's fence was made of, to tell when it is out of date (`actor::turn::fence`): a
/// hash of every matter's id and folder the host knows, and one of the thread's invites. In
/// memory only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct FenceKey {
    /// The matters' ids and folders, hashed.
    pub(crate) matters: u64,
    /// The thread's invites, hashed.
    pub(crate) invited: u64,
}

/// Every other matter a launch fences (the module doc).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct MatterFence {
    /// The other matters, sorted by id.
    pub(crate) others: Vec<Other>,
    /// What the launch made them of.
    pub(crate) key: FenceKey,
    /// The session's own matter folder, each spelling ([`Self::resolve`]).
    pub(crate) own: Vec<PathBuf>,
}

impl MatterFence {
    /// No other matter (a unit test's keys).
    #[cfg(test)]
    pub(crate) const NONE: Self = Self {
        others: Vec::new(),
        key: FenceKey {
            matters: 0,
            invited: 0,
        },
        own: Vec::new(),
    };

    /// The fence before the launch prep resolves it: `others`, sorted by id, so a launch's rules
    /// and the matters its shell rules keep (`agent::loads::profile`) do not depend on a map's
    /// order.
    pub(crate) fn asked(mut others: Vec<Other>) -> Self {
        others.sort_by_key(|other| other.id);
        Self {
            others,
            ..Self::default()
        }
    }

    /// Fills each matter's spellings and fence paths, the session's matter folder spelled each
    /// of `own` ways and `<data>` each of `data` ways. Reads the file system: off the actor.
    pub(crate) fn resolve(mut self, own: &[&Path], data: &[&Path]) -> Self {
        let net = NetEnv::current();
        let disk = Disk::resolved(&net);
        self.own = own.iter().map(|place| place.to_path_buf()).collect();
        for other in &mut self.others {
            other.spellings = spellings(&other.folder, &disk);
        }
        let invited: Vec<PathBuf> = self
            .others
            .iter()
            .filter(|other| other.invited)
            .flat_map(|other| other.spellings.iter().cloned())
            .collect();
        let mut kept_out = 0_usize;
        for other in &mut self.others {
            let mut keep: Vec<&Path> = own.iter().chain(data).copied().collect();
            if !other.invited {
                keep.extend(invited.iter().map(PathBuf::as_path));
            }
            let mut paths = Vec::new();
            for spelling in &other.spellings {
                match fence_paths(spelling, &keep, &net) {
                    Some(found) => paths.extend(found),
                    None => kept_out += 1,
                }
            }
            let mut seen = std::collections::HashSet::new();
            paths.retain(|path| seen.insert(path.clone()));
            other.paths = paths;
        }
        if kept_out > 0 {
            tracing::warn!(
                kept_out,
                "another matter's folder holds this one and was not fenced"
            );
        }
        self
    }

    /// The fence paths of the matters this session is not invited into.
    pub(crate) fn denied_reads(&self) -> impl Iterator<Item = &Path> {
        self.others
            .iter()
            .filter(|other| !other.invited)
            .flat_map(|other| other.paths.iter().map(PathBuf::as_path))
    }

    /// The fence paths of every other matter.
    pub(crate) fn denied_edits(&self) -> impl Iterator<Item = &Path> {
        self.others
            .iter()
            .flat_map(|other| other.paths.iter().map(PathBuf::as_path))
    }

    /// The folders, each spelling, of the matters this session is invited into.
    pub(crate) fn read_allowed(&self) -> impl Iterator<Item = &Path> {
        self.others
            .iter()
            .filter(|other| other.invited)
            .flat_map(|other| other.spellings.iter().map(PathBuf::as_path))
    }

    /// The folders, each spelling, of the matters this session is not invited into: Claude's
    /// sandbox denies their reads whole (it re-opens the session's own matter inside one).
    pub(crate) fn fenced_folders(&self) -> impl Iterator<Item = &Path> {
        self.others
            .iter()
            .filter(|other| !other.invited)
            .flat_map(|other| other.spellings.iter().map(PathBuf::as_path))
    }
}

/// `folder`'s spellings: where it leads (`touchable`'s walk), then as stored when that differs;
/// as stored alone when it is not absolute-and-local or does not resolve.
fn spellings(folder: &Path, disk: &Disk<'_>) -> Vec<PathBuf> {
    if !folder.is_absolute() {
        return Vec::new();
    }
    let mut out: Vec<PathBuf> = touchable(folder, folder, disk).into_iter().collect();
    if !out.iter().any(|walked| walked == folder) {
        out.push(folder.to_owned());
    }
    out
}

/// `path`'s names for a prefix comparison, folded where the file system folds.
pub(crate) fn key(path: &Path) -> Vec<OsString> {
    path.components()
        .map(|part| match part {
            Component::Normal(name) if FOLD_CASE => {
                OsString::from(fold_name(&name.to_string_lossy()))
            }
            other @ (Component::Normal(_)
            | Component::Prefix(_)
            | Component::RootDir
            | Component::CurDir
            | Component::ParentDir) => other.as_os_str().to_owned(),
        })
        .collect()
}

/// The fence paths of one spelling of another matter's folder (the module doc), kept places
/// `keep`: `None` when it is one of them, or holds one and can't be listed down to it.
fn fence_paths(spelling: &Path, keep: &[&Path], net: &NetEnv) -> Option<Vec<PathBuf>> {
    let at = key(spelling);
    let mut inside: Vec<Vec<OsString>> = Vec::new();
    for place in keep {
        let place_key = key(place);
        if !place_key.starts_with(&at) {
            continue;
        }
        if place_key.len() == at.len() {
            return None;
        }
        // Its names below `spelling`, as the place spells them.
        let below: Vec<OsString> = place
            .components()
            .skip(at.len())
            .map(|part| part.as_os_str().to_owned())
            .collect();
        inside.push(below);
    }
    if inside.is_empty() {
        return Some(vec![spelling.to_owned()]);
    }
    if net.is_network(spelling) {
        return None;
    }
    let mut out = Vec::new();
    down_to(spelling, &inside, &mut out)?;
    Some(out)
}

/// Every entry of `parent`, and of each folder on the way to each of `ways` (names below
/// `parent`), that is not on a way, into `out`; `None` when a folder on the way can't be listed.
fn down_to(parent: &Path, ways: &[Vec<OsString>], out: &mut Vec<PathBuf>) -> Option<()> {
    let fold = |name: &OsString| key(Path::new(name));
    let mut names: Vec<OsString> = std::fs::read_dir(parent)
        .ok()?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<_, _>>()
        .ok()?;
    names.sort();
    for name in names {
        let on_way: Vec<&Vec<OsString>> = ways
            .iter()
            .filter(|way| way.first().is_some_and(|first| fold(first) == fold(&name)))
            .collect();
        if on_way.is_empty() {
            out.push(parent.join(&name));
            continue;
        }
        // A kept place itself is left whole; a folder on the way is walked into.
        if on_way.iter().any(|way| way.len() == 1) {
            continue;
        }
        let tails: Vec<Vec<OsString>> = on_way
            .iter()
            .map(|way| way.iter().skip(1).cloned().collect())
            .collect();
        down_to(&parent.join(&name), &tails, out)?;
    }
    Some(())
}

/// The innermost other matter `path` lies in by name (each spelling compared folded), unless
/// the session's own matter (each spelling) lies deeper on the way: a refused path there is the
/// session's own. Paths only.
pub(crate) fn innermost<'a>(fence: &'a MatterFence, path: &Path) -> Option<&'a Other> {
    let at = key(path);
    let depth = |spelling: &Path| {
        let spelling = key(spelling);
        at.starts_with(&spelling).then_some(spelling.len())
    };
    let own_depth = fence.own.iter().filter_map(|place| depth(place)).max();
    let (other, deepest) = fence
        .others
        .iter()
        .filter_map(|other| {
            let deepest = other.spellings.iter().filter_map(|s| depth(s)).max()?;
            Some((other, deepest))
        })
        .max_by_key(|(_, deepest)| *deepest)?;
    (own_depth.is_none_or(|own| own < deepest)).then_some(other)
}

#[cfg(test)]
#[path = "matter_fence/tests.rs"]
mod tests;
