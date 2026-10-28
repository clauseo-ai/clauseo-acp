//! Every failure of this crate, and the code each one reaches a client as.

use std::io;
use std::path::PathBuf;

use clauseo_protocol::ErrorCode;

/// Why an agent did not start. A running agent never fails: how it ended is an
/// [`ExitReport`](crate::ExitReport), not an error.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The launch spec breaks a rule of [`LaunchSpec::new`](crate::LaunchSpec::new).
    #[error("invalid launch spec: {0}")]
    InvalidSpec(String),
    /// The OS could not start the program.
    #[error("could not start {}: {source}", program.display())]
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
}

impl Error {
    /// The code a client sees. Every error here is a failed launch.
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::InvalidSpec(_) | Error::Spawn { .. } | Error::Os { .. } => {
                ErrorCode::AgentLaunchFailed
            }
        }
    }
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
        ];
        for error in &errors {
            assert_eq!(error.code(), ErrorCode::AgentLaunchFailed, "{error}");
            assert_eq!(error.code().as_str(), "agent.launch_failed");
        }
    }
}
