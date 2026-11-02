//! Every failure of this crate, and the code each one reaches a client as.

use std::io;

use clauseo_protocol::ErrorCode;

/// Why a policy was refused, a look at the machine failed, or a jailed agent did not start.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The policy breaks one of [`PolicyBuilder::build`](crate::PolicyBuilder::build)'s rules,
    /// or [`Policy::check_allowed_folder`](crate::Policy::check_allowed_folder) refused a folder.
    /// The message names the rule and the path.
    #[error("sandbox policy refused: {0}")]
    Policy(String),
    /// An OS call that looks at the machine failed: the matter walk, or the sandbox probe.
    #[error("sandbox probe failed: {0}")]
    Probe(#[source] io::Error),
    /// The jailed agent did not start.
    #[error(transparent)]
    Launch(#[from] clauseo_process::Error),
    /// An egress proxy setting was refused: an allowlist entry or the upstream proxy's URL. The
    /// message names the setting and the rule.
    #[error("egress proxy setting refused: {0}")]
    ProxyConfig(String),
    /// The egress proxy did not start: a listener did not bind, the OS gave no randomness for
    /// the token, or its thread did not start.
    #[error("egress proxy did not start: {0}")]
    ProxyStart(#[source] io::Error),
}

impl Error {
    /// The code a client sees. A refused policy stops the launch, so it is a failed launch, as
    /// a launch error is, and so are the proxy's errors; a failed probe is unexpected, so it is
    /// `Internal`.
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::Policy(_) | Error::ProxyConfig(_) | Error::ProxyStart(_) => {
                ErrorCode::AgentLaunchFailed
            }
            Error::Probe(_) => ErrorCode::Internal,
            Error::Launch(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_match_protocol() {
        let launch = Error::Launch(clauseo_process::Error::InvalidSpec("relative".to_owned()));
        assert_eq!(launch.code(), ErrorCode::AgentLaunchFailed);
        let policy = Error::Policy("relative path".to_owned());
        assert_eq!(policy.code(), ErrorCode::AgentLaunchFailed);
        let probe = Error::Probe(io::Error::other("no sandbox-exec"));
        assert_eq!(probe.code(), ErrorCode::Internal);
        assert!(policy.to_string().contains("relative path"), "{policy}");
    }
}
