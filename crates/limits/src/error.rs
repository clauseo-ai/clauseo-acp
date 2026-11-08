//! The crate's one error type. A read never returns it: a reader turns it into a
//! [`ReadFailure`](crate::ReadFailure) with its text.

use std::io;
use std::path::PathBuf;

/// Why a reader could not be set up or prepared.
#[derive(Debug, thiserror::Error)]
pub(crate) enum Error {
    /// An agent's settings entry breaks a rule of the reader's paths. The text names the rule
    /// and the path, never an environment variable.
    #[error("agents.{agent}: {rule}")]
    Refused {
        /// The agent's settings id, such as `claude-acp`.
        agent: &'static str,
        /// The rule it breaks.
        rule: String,
    },
    /// The readers' run folder, or the `limits` folder above it, is there but is a link or not
    /// a folder.
    #[error("the readers' run folder {} {what}", path.display())]
    RunFolder {
        /// The folder.
        path: PathBuf,
        /// What is wrong with it.
        what: String,
    },
    /// Clauseo's Codex home is there but is a link or not a folder.
    #[error("the Codex home {} {what}", path.display())]
    CodexHome {
        /// The folder.
        path: PathBuf,
        /// What is wrong with it.
        what: String,
    },
    /// A file step failed.
    #[error("could not {what} {}: {source}", path.display())]
    Io {
        /// The step, such as `write`.
        what: &'static str,
        /// The file or folder.
        path: PathBuf,
        /// What the OS said.
        source: io::Error,
    },
}

impl Error {
    /// A fixed word for its kind, for a log line: never a path (decision record 0047 keeps
    /// paths out of `host.log`), as the text of every kind but `Refused` carries one, and a
    /// refused rule names the path too.
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            Error::Refused { .. } => "refused",
            Error::RunFolder { .. } => "run_folder",
            Error::CodexHome { .. } => "codex_home",
            Error::Io { .. } => "io",
        }
    }

    /// An [`Error::Io`] for `what` on `path`.
    pub(crate) fn io(what: &'static str, path: impl Into<PathBuf>, source: io::Error) -> Error {
        Error::Io {
            what,
            path: path.into(),
            source,
        }
    }
}
