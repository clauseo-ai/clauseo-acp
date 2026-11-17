//! Codex's pre-card path check: a command whose paths lead into another matter's folder is
//! refused before the lawyer sees a card, as one into Clauseo's data folder is
//! (`actor/turn/outside.rs`). Every path the request names (`FenceCheck::paths`, and its
//! command's words, `input::command_args`) is resolved the data fence's way
//! (relative to `rawInput.cwd`, through links, dangling ones included, `resolve.rs`) and compared
//! with each other matter's folder, resolved too, by text (case and Unicode form folded where the
//! volume ignores them) and, for a spelling the text misses (`/System/Volumes/Data/…`), by file
//! identity. A path in this matter's own folder counts only when it is in another matter nested
//! in it, never because this folder sits inside another's.
//!
//! Best effort: a script can build a path its words do not show (a variable, a here-doc, a
//! program that opens a file it computes). A path the walk cannot vouch for (a share, an
//! unreadable link) is refused: by the data-folder check for the request's paths, and here for
//! the command's words, which that check never sees, when a link on the word's way made the walk
//! fail, unless the word's own text is a network path, which it has judged. A word whose walk
//! failed on its own names (Windows' drive-relative `Q:`, a name under a file, a name in a
//! folder that can't be read) is not refused here. The Codex sandbox and the guard still stand
//! behind it.

use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use super::identity::DataIdentity;
use super::resolve::{self, Disk, Found};
use super::{FenceCheck, key};
use crate::network_paths::resolve::Entry;

/// A file's identity on its volume (`Fs::identity`).
type Id = (u64, u128);

/// The matter folders a Codex request's paths are checked against, and its command's words.
#[derive(Debug, Clone)]
pub(crate) struct OtherMatters {
    /// This matter's folder, as stored.
    pub(crate) own: PathBuf,
    /// Every other matter's folder the host knows now, as stored; one registered later is not
    /// checked.
    pub(crate) others: Vec<PathBuf>,
    /// The command's words that may name a file (`input::command_args`), bare names included.
    /// Checked here only, never by the data-folder check's stricter rules (a Windows name with a
    /// `:`, such as `TODO:`, would be refused there).
    pub(crate) args: Vec<PathBuf>,
    /// The agent launch's [`Cache`] of the matters' folders resolved.
    pub(crate) cache: Cache,
}

/// This and the other matters' folders resolved and their identities, kept for one launch so
/// each request does not walk every matter's folder again: read again when the matter list
/// (or this matter's folder) changes. Shared by the launch's requests.
#[derive(Debug, Clone, Default)]
pub(crate) struct Cache(Arc<Mutex<Option<Arc<Resolved>>>>);

impl Cache {
    /// The folders for `matters` as compared under `insensitive`: kept ones when they were read
    /// for the same folders, else `read`'s, kept.
    fn resolved(
        &self,
        matters: &OtherMatters,
        insensitive: bool,
        read: impl FnOnce() -> Resolved,
    ) -> Arc<Resolved> {
        let mut kept = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(kept) = kept.as_ref()
            && kept.own == matters.own
            && kept.others == matters.others
            && kept.insensitive == insensitive
        {
            return Arc::clone(kept);
        }
        let fresh = Arc::new(read());
        *kept = Some(Arc::clone(&fresh));
        fresh
    }
}

/// The matters' folders resolved (`Cache`).
#[derive(Debug)]
struct Resolved {
    /// This matter's folder as stored, the others', and the folding, which they were read for.
    own: PathBuf,
    others: Vec<PathBuf>,
    insensitive: bool,
    /// This matter's folder resolved, as a key, and its identity.
    own_key: Option<Vec<OsString>>,
    own_id: Option<Id>,
    /// Each other matter's folder that resolves, as a key.
    other_keys: Vec<Vec<OsString>>,
    /// Each other matter's folder that resolves and whose identity reads: its identity, and
    /// those of it and every folder above it that read.
    folders: Vec<(Id, Vec<Id>)>,
}

impl Resolved {
    /// `matters`' folders resolved on `disk` the data fence's way, and their identities.
    fn read(
        matters: &OtherMatters,
        insensitive: bool,
        fence: &FenceCheck,
        disk: &Disk<'_>,
    ) -> Self {
        let home = fence.home.as_deref();
        let real = |path: &PathBuf| match resolve::resolve(path, &fence.cwd, home, disk) {
            Found::Path(real) => Some(real),
            Found::Refused | Found::ShareMatter => None,
        };
        let id = |path: &Path| disk.fs.identity(path).ok();
        let own = real(&matters.own);
        let others: Vec<PathBuf> = matters.others.iter().filter_map(real).collect();
        let folders = others
            .iter()
            .filter_map(|folder| {
                let line = folder.ancestors().filter_map(id).collect();
                Some((id(folder)?, line))
            })
            .collect();
        Self {
            own: matters.own.clone(),
            others: matters.others.clone(),
            insensitive,
            own_key: own.as_ref().map(|own| key(own, insensitive)),
            own_id: own.as_deref().and_then(id),
            other_keys: others.iter().map(|other| key(other, insensitive)).collect(),
            folders,
        }
    }
}

impl OtherMatters {
    /// Whether the request reaches another matter's folder, or by one of [`Self::args`]
    /// Clauseo's data folder, comparing ignoring case when `insensitive`; stats the file system:
    /// - a path of `fence` or an arg that resolves into another matter's folder and outside this
    ///   matter's own (a matter nested in this one's folder still counts);
    /// - a search folder of `fence` (`grep -r`, `find`, `cp -r`) that holds another matter's
    ///   folder, such as `..`;
    /// - an arg that resolves into the data folder outside the session's own folders there
    ///   (`FenceCheck::own`: its run folder, its notes, the shared plugin folder): a bare name
    ///   through a link;
    /// - an arg the walk can't vouch for whose own text is no network path.
    pub(crate) fn reached(&self, fence: &FenceCheck, insensitive: bool, disk: &Disk<'_>) -> bool {
        let home = fence.home.as_deref();
        let found = |path: &PathBuf| resolve::resolve_all(path, &fence.cwd, home, disk);
        let id = |path: &Path| disk.fs.identity(path).ok();
        let matters = self.cache.resolved(self, insensitive, || {
            Resolved::read(self, insensitive, fence, disk)
        });
        let others = &matters.other_keys;
        let own = matters.own_key.as_ref();
        let own_id = matters.own_id;
        // A matter nested in this one's folder is still another matter.
        let counts = |other: &Vec<OsString>, outside_own: bool| {
            outside_own || own.is_some_and(|own| other.starts_with(own) && other != own)
        };
        let outside_own = |text: &Vec<OsString>| own.is_none_or(|own| !text.starts_with(own));
        // By identity too (device or volume, and file number), for a spelling the text misses:
        // `/System/Volumes/Data/…` (a firmlink), a hard-to-fold name. Only names that exist.
        let under = |path: &Path, target: Id| path.ancestors().any(|up| id(up) == Some(target));
        // Walking up from `path`: another matter met first counts; this matter met first ends it
        // (a matter nested in this one is met before this one).
        let met_other = |path: &Path| {
            path.ancestors().find_map(|up| {
                let found = id(up)?;
                if Some(found) == own_id {
                    Some(false)
                } else {
                    matters
                        .folders
                        .iter()
                        .any(|(other, _)| *other == found)
                        .then_some(true)
                }
            }) == Some(true)
        };
        let inside_other = |found: &Found| match found {
            Found::Path(path) => {
                let text = key(path, insensitive);
                let outside = outside_own(&text);
                others
                    .iter()
                    .any(|other| text.starts_with(other) && counts(other, outside))
                    || met_other(path)
            }
            Found::Refused | Found::ShareMatter => false,
        };
        let holds_other = |found: &Found| match found {
            Found::Path(root) => {
                let text = key(root, insensitive);
                let outside = outside_own(&text);
                let by_text = others
                    .iter()
                    .any(|other| other.starts_with(&text) && counts(other, outside));
                by_text
                    || id(root).is_some_and(|root_id| {
                        let root_in_own = own_id.is_some_and(|own| under(root, own));
                        matters.folders.iter().any(|(other, line)| {
                            let nested =
                                own_id.is_some_and(|own| *other != own && line.contains(&own));
                            line.contains(&root_id) && (!root_in_own || nested)
                        })
                    })
            }
            Found::Refused | Found::ShareMatter => false,
        };
        // The session's own folders in the data folder, as the data-folder check counts them
        // (`FenceCheck::own`: `allowed` and the shared plugin folder, each only at its place),
        // and the data folder's identity, as that check reads it (`DataIdentity`).
        let real_data = match resolve::resolve(&fence.data, &fence.cwd, home, disk) {
            Found::Path(real_data) => Some(real_data),
            Found::Refused | Found::ShareMatter => None,
        };
        let identity = real_data
            .as_deref()
            .map(|real_data| DataIdentity::capture(disk.fs, real_data, &fence.health));
        let data = real_data
            .as_ref()
            .zip(identity.as_ref())
            .map(|(real_data, identity)| {
                let own = fence.own(real_data, insensitive, disk, identity);
                (key(real_data, insensitive), own, identity)
            });
        // As the data-folder check judges a path: by text, else by identity, for a spelling the
        // text misses (`/System/Volumes/Data/…`).
        let inside_data = |found: &Found| match (found, &data) {
            (Found::Path(path), Some((data, own, identity))) => {
                let text = key(path, insensitive);
                if text.starts_with(data) {
                    !own.iter().any(|own| text.starts_with(own))
                } else {
                    identity.holds(path)
                }
            }
            _ => false,
        };
        // A word the walk can't vouch for because of a link on its way (a link to a share, one
        // that can't be read, a loop) is refused here, as the data-folder check refuses such a
        // path, unless its own text is a network path: the data-folder check has judged that
        // text already (`printf '\\n'` stays allowed). Not a drive-relative word (Windows'
        // `Q:` from `Select-String -Pattern "Q:"`, Python's `except … as e:`), which names no
        // folder to walk from, nor one whose walk failed on its own names (`contract.docx/word`,
        // a name in a folder that can't be read): those name no link that could lead elsewhere.
        // A relative word (no `rawInput.cwd` came, or a relative one, joined to the word) is
        // looked at from `fence.cwd`, as the walk took it, not from the host's own folder.
        let unjudged = |arg: &PathBuf, place: &Found| {
            matches!(place, Found::Refused)
                && !disk.net.is_network(arg)
                && !drive_relative(arg)
                && through_link(&fence.cwd.join(arg), &fence.cwd, disk)
        };
        fence
            .paths
            .iter()
            .any(|path| found(path).iter().any(&inside_other))
            || fence
                .roots
                .iter()
                .any(|root| found(root).iter().any(&holds_other))
            || self.args.iter().any(|arg| {
                found(arg)
                    .iter()
                    .any(|place| inside_other(place) || inside_data(place) || unjudged(arg, place))
            })
    }
}

/// Whether `path` is drive-relative (`Q:`, `C:x`: a drive and no root), as a Windows word of a
/// letter and a colon becomes when joined to a folder. Never on Unix.
fn drive_relative(path: &Path) -> bool {
    matches!(path.components().next(), Some(Component::Prefix(_))) && !path.has_root()
}

/// Whether a name on `path`'s way, below where it parts from `cwd`, is a link, looked at from
/// the top and stopping at the first link (nothing past a link is looked at by the system, which
/// would follow it) or the first name that can't be looked at, and never at a network path.
fn through_link(path: &Path, cwd: &Path, disk: &Disk<'_>) -> bool {
    let below: Vec<&Path> = path
        .ancestors()
        .take_while(|up| !up.as_os_str().is_empty() && !cwd.starts_with(up))
        .collect();
    for name in below.into_iter().rev() {
        if disk.net.is_network(name) {
            return false;
        }
        match disk.fs.entry(name) {
            Ok(Entry::Link) => return true,
            Ok(Entry::Dir | Entry::File) => {}
            Err(_) => return false,
        }
    }
    false
}

#[cfg(test)]
mod tests;
