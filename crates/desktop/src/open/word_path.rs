//! What `open_file` hands Word for a long path. Word on Windows cannot open a file whose full
//! path is over [`WORD_PATH_LIMIT`] UTF-16 units. Measured on Windows with Word for Office 365
//! and a 298-unit path: the 8.3 short form of the whole path opens, but Word titles the window
//! with the short leaf (`X8C7B~1`), so it is never used. The short form of the folders followed
//! by the real file name opens with the real name, and Save writes the long file in place; Word
//! starts in Protected View every time. Where the volume keeps no 8.3 names (many network shares,
//! or 8.3 creation off), `GetShortPathNameW` hands the long form back unchanged.
//!
//! So a Word document over the limit opens through its short folders and its real name when that
//! fits, and is refused as `matter.document_path_too_long` when it does not. Every other file, and
//! every path within the limit, opens as it is. [`word_open_path`] decides, with the folder
//! resolver passed in, so it is tested on every OS; [`short_folder`] is the real resolver, on
//! Windows only.

use std::path::{Path, PathBuf};

/// The longest full path Word on Windows opens, in UTF-16 units: `MAX_PATH` less its NUL.
pub(crate) const WORD_PATH_LIMIT: usize = 259;

/// The Word document types, lower case, without the dot. `docm` and `dotm` are not in
/// [`super::DOCUMENT_TYPES`], so `open_file` refuses them before this; they are named so the rule
/// holds if that list changes.
pub(crate) const WORD_TYPES: &[&str] = &["doc", "docx", "docm", "dotx", "dotm", "rtf"];

/// A Word document whose path stays over [`WORD_PATH_LIMIT`] even with its folders shortened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TooLong;

/// The path to open `path` by, and whether Word opens it in Protected View. A Word document (by
/// its extension, any case) over [`WORD_PATH_LIMIT`] units opens as `short_folder(parent)` joined
/// to its real file name when that is within the limit (`true`); [`TooLong`] when there is no
/// short form or it is still too long. The file name is never shortened. Anything else comes back
/// unchanged (`false`). A verbatim (`\\?\`) path is measured, and handed on, in its plain form.
pub(crate) fn word_open_path(
    path: &Path,
    short_folder: impl Fn(&Path) -> Option<PathBuf>,
) -> Result<(PathBuf, bool), TooLong> {
    if !is_word(path) || units(&plain(path)) <= WORD_PATH_LIMIT {
        return Ok((path.to_path_buf(), false));
    }
    let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(TooLong);
    };
    let short = plain(&short_folder(parent).ok_or(TooLong)?.join(name));
    if units(&short) <= WORD_PATH_LIMIT {
        Ok((short, true))
    } else {
        Err(TooLong)
    }
}

/// Whether `path` names a Word document by its extension, in any case.
fn is_word(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| WORD_TYPES.contains(&ext.to_ascii_lowercase().as_str()))
}

/// The length of `path` in UTF-16 units, as Windows counts it.
fn units(path: &Path) -> usize {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt as _;
        path.as_os_str().encode_wide().count()
    }
    #[cfg(not(windows))]
    {
        path.as_os_str().to_string_lossy().encode_utf16().count()
    }
}

/// `path` without a verbatim prefix: `\\?\C:\a` as `C:\a`, `\\?\UNC\server\share` as
/// `\\server\share`. A path without one, or not Unicode, comes back as it is.
fn plain(path: &Path) -> PathBuf {
    path.to_str()
        .and_then(plain_str)
        .map_or_else(|| path.to_path_buf(), PathBuf::from)
}

/// [`plain`] on text: `None` when there is no verbatim prefix to drop.
fn plain_str(path: &str) -> Option<String> {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        Some(format!(r"\\{rest}"))
    } else {
        path.strip_prefix(r"\\?\").map(str::to_owned)
    }
}

/// The 8.3 short form of the folder `folder` (`GetShortPathNameW`): `None` when Windows gives
/// none. On a volume without 8.3 names it is the long form, which [`word_open_path`] then finds
/// still too long.
#[cfg(windows)]
#[expect(unsafe_code, reason = "FFI to GetShortPathNameW")]
pub(crate) fn short_folder(folder: &Path) -> Option<PathBuf> {
    use std::ffi::OsString;
    use std::os::windows::ffi::{OsStrExt as _, OsStringExt as _};

    use windows::Win32::Storage::FileSystem::GetShortPathNameW;
    use windows::core::PCWSTR;

    let wide: Vec<u16> = folder.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut buffer = vec![0u16; 1024];
    loop {
        // SAFETY: `wide` is NUL-terminated and outlives the call; the call writes at most
        // `buffer.len()` units into `buffer`.
        let written = unsafe { GetShortPathNameW(PCWSTR(wide.as_ptr()), Some(&mut buffer)) };
        let written = usize::try_from(written).ok()?;
        if written == 0 {
            return None;
        }
        if written < buffer.len() {
            buffer.truncate(written);
            return Some(PathBuf::from(OsString::from_wide(&buffer)));
        }
        // Too small: `written` is the size needed, NUL included.
        buffer.resize(written, 0);
    }
}

#[cfg(test)]
mod tests;
