//! Every failure of this crate, and the code each one reaches a client as.

use std::io;
use std::path::PathBuf;

use clauseo_protocol::ErrorCode;

/// Why an agent did not start. A running agent never fails: how it ended is an
/// [`ExitReport`](crate::ExitReport), not an error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The launch spec breaks a rule of [`LaunchSpec::new`](crate::LaunchSpec::new). The text
    /// names the rule, never the path.
    #[error("invalid launch spec: {0}")]
    InvalidSpec(String),
    /// The spec's working folder is not there, or is not a folder
    /// ([`LaunchSpec::new`](crate::LaunchSpec::new)). Kept apart from [`Error::InvalidSpec`] so a
    /// caller can say the folder is gone (the host: `matter.folder_missing`). No path.
    #[error("invalid launch spec: the working folder is not there, or is not a folder")]
    NoWorkingFolder,
    /// The OS could not start the program. The text names the program's file name, never its
    /// folder (which can hold the user's name).
    #[error("could not start {}: {source}", file_name(program))]
    Spawn {
        /// The program the spec named.
        program: PathBuf,
        /// What the OS said.
        source: io::Error,
    },
    /// The program started, but its process tree could not be set up, so it was ended again.
    #[error("{what}: {source}")]
    Os {
        /// The step that failed.
        what: &'static str,
        /// What the OS said.
        source: io::Error,
    },
    /// The process is exiting: [`close_spawns`](crate::close_spawns) closed the gate, so nothing
    /// was started.
    #[error("the app is exiting: no new process starts")]
    Closing,
    /// macOS: the Mac lets one user run so few processes (`kern.maxprocperuid`) that the Safe
    /// workspace cannot hold back its reserve for the lawyer's own apps and still leave the agent
    /// room, so the agent was not started.
    #[error(
        "This Mac's process limit is too low for Clauseo's Safe workspace: it lets one user run \
         {per_user} programs at once, and the Safe workspace needs at least {needed}."
    )]
    ProcessLimitTooLow {
        /// `kern.maxprocperuid`, as read.
        per_user: u64,
        /// The least the launcher accepts.
        needed: u64,
    },
}

impl Error {
    /// The code a client sees. Every error here is a failed launch.
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::InvalidSpec(_)
            | Error::NoWorkingFolder
            | Error::Spawn { .. }
            | Error::Os { .. }
            | Error::Closing
            | Error::ProcessLimitTooLow { .. } => ErrorCode::AgentLaunchFailed,
        }
    }
}

/// `program`'s file name, for [`Error::Spawn`]'s text: never the folders above it.
fn file_name(program: &std::path::Path) -> String {
    program.file_name().map_or_else(
        || "the program".to_owned(),
        |name| name.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_protocol() {
        let errors = [
            Error::InvalidSpec("relative program".to_owned()),
            Error::Spawn {
                program: PathBuf::from("/missing"),
                source: io::Error::from(io::ErrorKind::NotFound),
            },
            Error::Os {
                what: "could not create a job object",
                source: io::Error::other("denied"),
            },
            Error::Closing,
            Error::ProcessLimitTooLow {
                per_user: 600,
                needed: 1024,
            },
        ];
        for error in &errors {
            assert_eq!(error.code(), ErrorCode::AgentLaunchFailed, "{error}");
            assert_eq!(error.code().as_str(), "agent.launch_failed");
        }
    }
}
