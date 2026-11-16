//! A folder's spellings on the disk beside the ones the launch has: its canonical path (case
//! and Unicode form as the disk keeps them), which the rules name, and, on macOS, its twin on
//! the data volume (`/System/Volumes/Data/Users/…` for `/Users/…` and back, a firmlink no
//! walk sees) when it is the same folder, which only comparisons use. Then the keys of a match
//! that keeps a place out of a deny, folded as the file system folds names
//! (`clauseo_matters::Fold::FileSystem`), never wider: `fold_name`'s safety fold would take
//! `Zoë` (NFD) for `Zoë` (NFC), two folders on NTFS.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use clauseo_matters::{Fold, keys};

use super::real;
use crate::network_paths::resolve::Disk;

/// macOS's data volume: the root and where its firmlinked folders also are
/// (`/System/Volumes/Data`); none elsewhere.
pub(crate) type DataVolume<'a> = Option<(&'a Path, &'a Path)>;

/// This system's data volume ([`DataVolume`]).
pub(crate) fn data_volume() -> DataVolume<'static> {
    cfg!(target_os = "macos").then(|| (Path::new("/"), Path::new("/System/Volumes/Data")))
}

/// `path`'s twin by its text on the data volume `(top, data)`: `top/x` for `data/x`, `data/x`
/// for `top/x`; none for either root itself.
pub(crate) fn twin_text(path: &Path, (top, data): (&Path, &Path)) -> Option<PathBuf> {
    if let Ok(rest) = path.strip_prefix(data) {
        return (!rest.as_os_str().is_empty()).then(|| top.join(rest));
    }
    let rest = path.strip_prefix(top).ok()?;
    (!rest.as_os_str().is_empty() && !data.starts_with(path)).then(|| data.join(rest))
}

/// Where the local, absolute `path` leads, canonicalised (case and Unicode form as the disk
/// keeps them), after the walk vouched for every link on the way, so no share is touched; none
/// on a share, for a path that is not absolute, or when it is `path` itself.
pub(crate) fn canonical(path: &Path, disk: &Disk<'_>) -> Option<PathBuf> {
    if !path.is_absolute() || disk.net.is_network(path) {
        return None;
    }
    let found = dunce::canonicalize(real(path, disk)?).ok()?;
    (found != path && !disk.net.is_network(&found)).then_some(found)
}

/// The data-volume twin of `path` ([`twin_text`]) when it is the same folder by identity; for
/// comparisons only, never a rule: a rule per twin would double every Mac's rules.
pub(crate) fn twin(path: &Path, disk: &Disk<'_>, volume: DataVolume<'_>) -> Option<PathBuf> {
    if disk.net.is_network(path) {
        return None;
    }
    let twin = twin_text(path, volume?)?;
    same(disk, path, &twin).then_some(twin)
}

/// Whether `a` and `b` are one file by their identities (device and file number).
fn same(disk: &Disk<'_>, a: &Path, b: &Path) -> bool {
    matches!((disk.fs.identity(a), disk.fs.identity(b)), (Ok(x), Ok(y)) if x == y)
}

/// `path`'s keys for a match that keeps a place out of a deny: names folded as this OS's file
/// system finds them, on macOS a data-volume path also without `/System/Volumes/Data`.
fn keep_keys(path: &Path) -> Vec<Vec<OsString>> {
    keys(path, Fold::FileSystem)
}

/// The names of `place` below `folder`, as `place` spells them, when a key of `place` starts
/// with one of `folder`'s ([`keep_keys`]): empty when they are the same folder.
pub(crate) fn below_keep(place: &Path, folder: &Path) -> Option<Vec<OsString>> {
    let (place_keys, folder_keys) = (keep_keys(place), keep_keys(folder));
    let rest = place_keys
        .iter()
        .flat_map(|place_key| {
            folder_keys
                .iter()
                .filter(|folder_key| place_key.starts_with(folder_key))
                .map(|folder_key| place_key.len() - folder_key.len())
        })
        .min()?;
    let names: Vec<OsString> = place
        .components()
        .map(|part| part.as_os_str().to_owned())
        .collect();
    names
        .get(names.len().saturating_sub(rest)..)
        .map(<[OsString]>::to_vec)
}

/// Whether the names `a` and `b` are one name to the file system ([`keep_keys`]).
pub(crate) fn same_name(a: &OsString, b: &OsString) -> bool {
    keep_keys(Path::new(a)) == keep_keys(Path::new(b))
}
