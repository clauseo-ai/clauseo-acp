//! The crate's one error type.

use std::io;
use std::path::PathBuf;

/// What can go wrong finding a folder, reading a policy source, writing `settings.json`, or
/// changing a setting that policy locks.
///
/// It never reaches a client as it is: `host` maps it to an error code.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    /// Reading or writing a file failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file.
        path: PathBuf,
        /// What the OS said.
        #[source]
        source: io::Error,
    },
    /// A JSON file does not parse. `line` and `column` count from 1, except that an error at
    /// the end of the input has column 0, as the JSON parser reports it.
    #[error("{}:{line}:{column}: {message}", path.display())]
    Parse {
        /// The file.
        path: PathBuf,
        /// The line of the problem.
        line: usize,
        /// The column of the problem, in bytes.
        column: usize,
        /// What the parser said.
        message: String,
    },
    /// A change names a key that policy set, so the user cannot change it. Nothing was written.
    #[error("{key} is set by policy and cannot be changed")]
    Locked {
        /// The dotted key, such as `ui.theme`.
        key: String,
    },
    /// The OS gave no path for a folder this crate needs.
    #[error("the OS gives no {folder} folder")]
    NoFolder {
        /// The folder, as the OS calls it (`home`, `LocalAppData`).
        folder: &'static str,
    },
    /// `CLAUSEO_DATA_DIR` is set to a path that is not absolute.
    #[error("CLAUSEO_DATA_DIR must be an absolute path, not {}", path.display())]
    RelativeDataDir {
        /// The value it holds.
        path: PathBuf,
    },
    /// Reading the Windows registry failed.
    #[error("registry key HKLM\\{key}: Win32 error {code}")]
    Registry {
        /// The key under `HKEY_LOCAL_MACHINE`.
        key: String,
        /// The Win32 error code.
        code: u32,
    },
}
