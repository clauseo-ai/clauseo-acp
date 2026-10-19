//! The one error type of the library. It never reaches a Clauseo client, so it maps to no
//! `ErrorCode`.

use std::path::PathBuf;

/// Why a scenario failed to load or validate, or why the binary was not found.
///
/// Its `Display` text has no `fake_agent: ` prefix; the binary adds it.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The scenario text is not a valid scenario: bad JSON, an unknown step, a broken label.
    #[error("{file}:{line}: {message}")]
    Load {
        /// File name of the scenario or included file, such as `prompt-stream.jsonl`.
        file: String,
        /// 1-based line in that file.
        line: usize,
        /// What is wrong.
        message: String,
    },
    /// A message the scenario sends does not survive the SDK's typed schema for its method.
    #[error("{file}:{line}: {method}: {message}")]
    Validate {
        /// File name of the scenario or included file.
        file: String,
        /// 1-based line in that file.
        line: usize,
        /// The JSON-RPC method of the message, such as `session/update`.
        method: String,
        /// The first JSON pointer that differs, and how.
        message: String,
    },
    /// Reading a scenario file or folder failed.
    #[error("{}: {source}", path.display())]
    Io {
        /// The file or folder.
        path: PathBuf,
        /// The error from the operating system.
        source: std::io::Error,
    },
    /// `std::env::current_exe` failed, so [`crate::bin_path`] cannot find the profile folder.
    #[error("cannot find the running executable (std::env::current_exe): {source}")]
    CurrentExe {
        /// The error from the operating system.
        source: std::io::Error,
    },
    /// `CLAUSEO_FAKE_AGENT_BIN` names a file that does not exist.
    #[error("{}: no such file, named by CLAUSEO_FAKE_AGENT_BIN", path.display())]
    BinaryVarNotFound {
        /// The path the variable holds.
        path: PathBuf,
    },
    /// The `fake_agent` binary is not in the target profile folder [`crate::bin_path`] looked in.
    #[error("{}: no such file; run `cargo build -p clauseo-fake_agent`", path.display())]
    BinaryNotFound {
        /// Where it looked.
        path: PathBuf,
    },
}
