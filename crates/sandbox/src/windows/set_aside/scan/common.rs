//! The folders a git folder's `commondir` names (`scan::Kept::Common`): the `commondir` read
//! relative to the git folder's handle, as itself and never waiting on an oplock, and what git
//! reads from each folder it names.

use std::ffi::{OsStr, OsString};
use std::io;
use std::path::{Path, PathBuf};

use super::super::files::io_failure;
use super::super::names::Key;
use super::super::repo_names::{COMMON_NAMES, is_git};
use super::super::run::Volume;
use super::{Found, Item, Kept, Now, Scan, Step, found, plain};

/// The most of a `commondir` file read: git reads one line.
const COMMONDIR_MAX: u64 = 4096;

/// The key below `root` of the folder the `commondir` file `name` in the folder at `folder`
/// (opened as `handle`) names (absolute, or relative to `folder`, by its words alone, never
/// followed), when it lies in the matter outside every `.git`: one outside the matter isn't
/// watched (a limit), and one in a `.git` is write-denied with it. The file is read relative to
/// `handle`, as itself, never waiting on an oplock ([`Volume::read_small`]); one that can't be
/// read names nothing, and its folder is still the git folder it is.
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
    let target = folder.join(line);
    let (target, root) = (plain(&target), plain(root));
    let inside = target.len() >= root.len()
        && target
            .iter()
            .zip(&root)
            .all(|(a, b)| a.eq_ignore_ascii_case(b));
    if !inside {
        tracing::debug!("a commondir names a folder outside the matter; not watched");
        return None;
    }
    let key: Key = target
        .into_iter()
        .skip(root.len())
        .map(OsString::from)
        .collect();
    (!key.iter().any(|part| is_git(part))).then_some(key)
}

/// For each folder a `commondir` names ([`Scan::common`]): each of [`COMMON_NAMES`] there, or
/// the folder itself while it is missing, as a [`Kept::Common`] item.
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
        let at_of = |path: &Path| match volume.file_id(path) {
            Ok(None) => Now::Absent,
            Ok(Some(id)) => Now::At(id),
            Err(error) if error.kind() == io::ErrorKind::PermissionDenied => Now::Shut,
            Err(error) => Now::Failed(io_failure(&error)),
        };
        let item = |key: Key, path: PathBuf, now: Now| Item {
            within: key
                .get(..key.len().saturating_sub(1))
                .unwrap_or_default()
                .to_vec(),
            key,
            path,
            through: Vec::new(),
            now,
            kept: Kept::Common,
        };
        let here = at_of(&path);
        if here == Now::Absent {
            if !target.is_empty() {
                found(scan, visit, item(target.clone(), path, Now::Absent));
            }
            continue;
        }
        for name in COMMON_NAMES {
            let now = at_of(&path.join(name));
            if now != Now::Absent {
                let mut key = target.clone();
                key.push(OsString::from(name));
                found(scan, visit, item(key, path.join(name), now));
            }
        }
    }
}
