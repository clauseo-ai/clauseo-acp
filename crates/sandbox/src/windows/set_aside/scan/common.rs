//! The folders a git folder's `commondir` names (`scan::Kept::Common`): the `commondir` read
//! relative to the git folder's handle, as itself and never waiting on an oplock; the folder it
//! names told inside the matter by identity (`locate.rs`); and what git reads from each such
//! folder, read through handles from the matter's folder (`reach.rs`).

use std::ffi::{OsStr, OsString};
use std::path::Path;

use super::super::names::Key;
use super::super::repo_names::{COMMON_NAMES, is_git};
use super::super::run::Volume;
use super::{Found, Item, Kept, Last, Missed, Now, Scan, Step};
use super::{found, locate, open_key, probe, unread};

/// The most of a `commondir` file read: git reads one line.
const COMMONDIR_MAX: u64 = 4096;

/// The key below `root` of the folder the `commondir` file `name` in the folder at `folder`
/// (opened as `handle`) names (absolute, or relative to `folder`, by its words, never opened
/// through a link), when it lies in the matter outside every `.git`, as [`locate`] tells it by
/// identity: one outside the matter isn't watched (a limit), and one in a `.git` is write-denied
/// with it. Its last part may be missing (a folder not made yet, watched as missing). The file
/// is read relative to `handle`, as itself, never waiting on an oplock
/// ([`Volume::read_small`]); one that can't be read names nothing, and its folder is still the
/// git folder it is.
pub(super) fn common_target<V: Volume>(
    volume: &V,
    handle: &V::Folder,
    (root, folder): (&Path, &Path),
    name: &OsStr,
) -> Option<Key> {
    let text = match volume.read_small(handle, name, COMMONDIR_MAX) {
        Ok(bytes) => String::from_utf8(bytes).ok()?,
        Err(error) => {
            let error = error.kind();
            tracing::debug!(%error, "a commondir that can't be read; it names nothing");
            return None;
        }
    };
    let line = text.lines().next()?.trim();
    if line.is_empty() {
        return None;
    }
    let Some(key) = locate(volume, root, (folder, Path::new(line)), Last::MayMiss) else {
        tracing::debug!("a commondir names a folder outside the matter; not watched");
        return None;
    };
    (!key.iter().any(|part| is_git(part))).then_some(key)
}

/// For each folder a `commondir` names ([`Scan::common`]): each of [`COMMON_NAMES`] there, or
/// the folder itself while it is missing, as a [`Kept::Common`] item. Each folder is opened
/// from `root` through handles, as itself; one past a link now is passed over (the link, new,
/// is the watch's to judge), and one that can't be opened makes each name unread.
pub(super) fn common_items<V: Volume>(
    volume: &V,
    root: &Path,
    scan: &mut Scan,
    visit: &mut dyn FnMut(Found<'_>) -> Step,
) {
    for target in scan.common.clone() {
        let path = target
            .iter()
            .fold(root.to_owned(), |at, part| at.join(part));
        let item = |name: Option<&str>, now: Now| {
            let mut key = target.clone();
            key.extend(name.map(OsString::from));
            Item {
                within: key
                    .get(..key.len().saturating_sub(1))
                    .unwrap_or_default()
                    .to_vec(),
                path: name.map_or_else(|| path.clone(), |name| path.join(name)),
                key,
                through: Vec::new(),
                now,
                kept: Kept::Common,
            }
        };
        let folder = match open_key(volume, root, &target) {
            Ok(folder) => folder,
            Err(Missed::Gone) => {
                if !target.is_empty() {
                    found(scan, visit, item(None, Now::Absent));
                }
                continue;
            }
            Err(Missed::Link) => {
                tracing::debug!("a folder a commondir names is past a link now; passed over");
                continue;
            }
            Err(Missed::Failed(error)) => {
                for name in COMMON_NAMES {
                    found(scan, visit, item(Some(name), unread(&error)));
                }
                continue;
            }
        };
        for name in COMMON_NAMES {
            let now = probe(volume, &folder, OsStr::new(name));
            if now != Now::Absent {
                found(scan, visit, item(Some(name), now));
            }
        }
    }
}
