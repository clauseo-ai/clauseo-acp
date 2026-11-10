//! The path comparisons every rule uses: [`under`] and [`same`], on paths put in one [`normal`]
//! form first.

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use super::names;

/// Whether `path` is `prefix` or under it, both first in the form [`normal`] gives. On macOS
/// names compare one by one by their [`names::name_key`], Unicode canonical caseless matching,
/// the folding APFS documents for its case-insensitive volumes (`DATA/x` is under `data`, an
/// NFD name matches its NFC form; the pairs checked against this Mac's volume are in the
/// tests). Elsewhere the comparison is exact: NTFS and Linux file systems keep NFC and NFD
/// names as two files.
pub(crate) fn under(path: &Path, prefix: &Path) -> bool {
    let (path, prefix) = (normal(path), normal(prefix));
    if path.starts_with(&*prefix) {
        true
    } else if cfg!(target_os = "macos") {
        let mut names = path.components();
        prefix.components().all(|want| {
            names
                .next()
                .is_some_and(|got| names::same_name(got.as_os_str(), want.as_os_str()))
        })
    } else {
        false
    }
}

/// Whether `a` and `b` are the same path, by [`under`]'s comparison.
pub(crate) fn same(a: &Path, b: &Path) -> bool {
    normal(a).components().count() == normal(b).components().count() && under(a, b)
}

/// `path` in the one form the comparisons use: a Windows verbatim drive or UNC path without its
/// `\\?\` ([`strip_verbatim`]), and on macOS the data volume's spelling as `/`
/// ([`strip_data_volume`]). Any other path is kept as it is.
fn normal(path: &Path) -> Cow<'_, Path> {
    if cfg!(windows)
        && let Some(plain) = strip_verbatim(path)
    {
        return Cow::Owned(plain);
    }
    if cfg!(target_os = "macos")
        && let Some(plain) = strip_data_volume(path)
    {
        return Cow::Owned(plain);
    }
    Cow::Borrowed(path)
}

/// `path` without its Windows verbatim prefix when that is a drive (`\\?\C:\x` is `C:\x`) or a
/// UNC share (`\\?\UNC\server\share\x` is `\\server\share\x`); `None` for any other path. Kept
/// separate from [`normal`] so the tests run on every OS.
pub(crate) fn strip_verbatim(path: &Path) -> Option<PathBuf> {
    let text = path.to_str()?;
    if let Some(unc) = text.strip_prefix(r"\\?\UNC\") {
        return Some(PathBuf::from(format!(r"\\{unc}")));
    }
    let drive = text.strip_prefix(r"\\?\")?;
    let mut chars = drive.chars();
    let (letter, colon) = (chars.next()?, chars.next()?);
    (letter.is_ascii_alphabetic() && colon == ':').then(|| PathBuf::from(drive))
}

/// `path` as `/…` when it is under the macOS data volume's spelling (`/System/Volumes/Data/Users`
/// is `/Users`, the same folder through the firmlink); `None` otherwise. Kept separate from
/// [`normal`] so the tests run on every OS.
pub(crate) fn strip_data_volume(path: &Path) -> Option<PathBuf> {
    let rest = path.strip_prefix("/System/Volumes/Data").ok()?;
    Some(Path::new("/").join(rest))
}
