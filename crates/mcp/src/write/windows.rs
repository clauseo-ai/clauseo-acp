//! Windows: the download move that never replaces a file, `MoveFileExW` without
//! `MOVEFILE_REPLACE_EXISTING` (std has none: `fs::rename` replaces, and `fs::hard_link` plus
//! `fs::remove_file` would leave two names while Chrome or a scanner holds the file open).
#![expect(
    unsafe_code,
    reason = "MoveFileExW, for a download move that never replaces a file"
)]

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows::Win32::Foundation::WIN32_ERROR;
use windows::Win32::Storage::FileSystem::{MOVE_FILE_FLAGS, MoveFileExW};
use windows::core::PCWSTR;

/// Moves `from` to `to`; `AlreadyExists` if `to` exists, a sharing violation while the file
/// is open without delete sharing.
pub(crate) fn move_new(from: &Path, to: &Path) -> io::Result<()> {
    let (from, to) = (wide(from)?, wide(to)?);
    // SAFETY: both pointers are to NUL-terminated UTF-16 strings that live until the call
    // returns; no flag is set, so the call fails rather than replace `to`.
    unsafe {
        MoveFileExW(
            PCWSTR(from.as_ptr()),
            PCWSTR(to.as_ptr()),
            MOVE_FILE_FLAGS(0),
        )
    }
    .map_err(|error| io_error(&error))
}

/// The Win32 error inside `error` as std's own OS error, so `kind()` is right:
/// `ERROR_ALREADY_EXISTS` (183) and `ERROR_FILE_EXISTS` (80) read as `AlreadyExists`.
/// `io::Error::from` would keep the HRESULT (`0x800700B7`), whose kind std does not know.
fn io_error(error: &windows::core::Error) -> io::Error {
    match WIN32_ERROR::from_error(error).and_then(|code| i32::try_from(code.0).ok()) {
        Some(code) => io::Error::from_raw_os_error(code),
        None => io::Error::from(error.clone()),
    }
}

/// `path` made absolute and normalised (`std::path::absolute`: `.`, `..` and `/` resolved, as
/// Windows itself would), as a NUL-terminated wide string with the `\\?\` prefix (`\\?\UNC\`
/// for a share), so a long path still works. A path that has the prefix already is kept.
fn wide(path: &Path) -> io::Result<Vec<u16>> {
    let full = std::path::absolute(path)?;
    let text = full.as_os_str();
    let (prefix, rest): (&str, &OsStr) = match full.to_str() {
        Some(s) if s.starts_with(r"\\?\") || s.starts_with(r"\\.\") => ("", text),
        Some(s) if let Some(share) = s.strip_prefix(r"\\") => (r"\\?\UNC\", OsStr::new(share)),
        Some(s) if s.as_bytes().get(1) == Some(&b':') => (r"\\?\", text),
        _ => ("", text),
    };
    Ok(OsStr::new(prefix)
        .encode_wide()
        .chain(rest.encode_wide())
        .chain(Some(0))
        .collect())
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;

    use windows::Win32::Foundation::{
        ERROR_ACCESS_DENIED, ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS,
    };

    use super::{io_error, move_new, wide};

    /// The review's M2: both "exists" errors read as `AlreadyExists`, which the download mover
    /// retries under the next name; another error keeps its own kind.
    #[test]
    fn an_existing_name_reads_as_already_exists() {
        for code in [ERROR_ALREADY_EXISTS, ERROR_FILE_EXISTS] {
            let error = windows::core::Error::from(code.to_hresult());
            assert_eq!(
                io_error(&error).kind(),
                io::ErrorKind::AlreadyExists,
                "{code:?}"
            );
        }
        let denied = windows::core::Error::from(ERROR_ACCESS_DENIED.to_hresult());
        assert_eq!(io_error(&denied).kind(), io::ErrorKind::PermissionDenied);
    }

    /// A real move onto a file that exists fails with `AlreadyExists` and changes neither file.
    #[test]
    fn a_move_onto_a_file_fails_with_already_exists() {
        let dir = std::env::temp_dir().join(format!("clauseo-mcp-move-{}", std::process::id()));
        if dir.exists() {
            crate::write::remove_dir_all(&dir).unwrap();
        }
        crate::write::create_dir_all(&dir).unwrap();
        let (from, to) = (dir.join("from.txt"), dir.join("to.txt"));
        crate::write::write(&from, "download").unwrap();
        crate::write::write(&to, "kept").unwrap();
        let error = move_new(&from, &to).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists, "{error}");
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "kept");
        assert_eq!(std::fs::read_to_string(&from).unwrap(), "download");
        crate::write::remove_dir_all(&dir).unwrap();
    }

    /// `.` and `..` are resolved before the prefix, which turns both off in Windows' own parse.
    #[test]
    fn a_path_is_normalised_before_its_prefix() {
        let text = |path: &str| String::from_utf16(&wide(Path::new(path)).unwrap()).unwrap();
        assert_eq!(text(r"C:\a\.\b\..\c.txt"), "\\\\?\\C:\\a\\c.txt\0");
        assert_eq!(text("C:/a/b.txt"), "\\\\?\\C:\\a\\b.txt\0");
        assert_eq!(
            text(r"\\host\share\a.txt"),
            "\\\\?\\UNC\\host\\share\\a.txt\0"
        );
        assert_eq!(text(r"\\?\C:\a.txt"), "\\\\?\\C:\\a.txt\0");
    }
}
