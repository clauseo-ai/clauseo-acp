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
}

impl Error {
    /// The code a client sees: a program the OS did not find is `agent.program_missing`, one it
    /// refused to run `agent.program_blocked`; anything else a failed launch.
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::Spawn { source, .. } => spawn_code(source, cfg!(windows)),
            Error::InvalidSpec(_) | Error::NoWorkingFolder | Error::Os { .. } | Error::Closing => {
                ErrorCode::AgentLaunchFailed
            }
        }
    }
}

/// Windows' refusals to run a program that have no `io::ErrorKind`: "blocked by group policy"
/// (`ERROR_ACCESS_DISABLED_BY_POLICY`, 1260: allow lists and software restriction), its
/// antivirus refusals (`ERROR_VIRUS_INFECTED` 225, `ERROR_VIRUS_DELETED` 226), and code
/// integrity: App Control for Business (4551) and Smart App Control (4553 to 4556). The code
/// integrity numbers are to confirm on win-02.
const WINDOWS_BLOCKED: [i32; 8] = [1260, 225, 226, 4551, 4553, 4554, 4555, 4556];

/// The code of a spawn the OS answered with `source`, on Windows when `windows`.
fn spawn_code(source: &io::Error, windows: bool) -> ErrorCode {
    let policy = windows
        && source
            .raw_os_error()
            .is_some_and(|code| WINDOWS_BLOCKED.contains(&code));
    let kind = source.kind();
    if kind == io::ErrorKind::NotFound {
        ErrorCode::AgentProgramMissing
    } else if kind == io::ErrorKind::PermissionDenied || policy {
        ErrorCode::AgentProgramBlocked
    } else {
        ErrorCode::AgentLaunchFailed
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
        let spawn = |source: io::Error| Error::Spawn {
            program: PathBuf::from("/agent"),
            source,
        };
        let missing = spawn(io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(missing.code(), ErrorCode::AgentProgramMissing, "{missing}");
        let denied = spawn(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(denied.code(), ErrorCode::AgentProgramBlocked, "{denied}");
        // Windows' policy and antivirus refusals, by their raw code alone.
        for code in WINDOWS_BLOCKED {
            let source = io::Error::from_raw_os_error(code);
            assert_eq!(
                spawn_code(&source, true),
                ErrorCode::AgentProgramBlocked,
                "{code}"
            );
        }
        // Elsewhere the same numbers are other errors: a failed launch.
        let other = io::Error::other("busy");
        assert_eq!(spawn_code(&other, true), ErrorCode::AgentLaunchFailed);
        let errors = [
            Error::InvalidSpec("relative program".to_owned()),
            Error::Spawn {
                program: PathBuf::from("/busy"),
                source: io::Error::from(io::ErrorKind::OutOfMemory),
            },
            Error::Os {
                what: "could not create a job object",
                source: io::Error::other("denied"),
            },
            Error::Closing,
        ];
        for error in &errors {
            assert_eq!(error.code(), ErrorCode::AgentLaunchFailed, "{error}");
            assert_eq!(error.code().as_str(), "agent.launch_failed");
        }
    }
}
