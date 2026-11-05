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
    /// Windows tier B can't prepare a folder it would grant on, or a folder it must close to
    /// this matter (`windows::admit`): the folder check answered other than `Ok`, or failed.
    /// Nothing was planned or written. The error names the kind of folder, never its path, so
    /// a matter's name reaches no log through it.
    #[error("the sandbox can't prepare {path_kind}: {support}")]
    FolderUnsupported {
        /// Which folder: the matter, the state folder, the notes folder and so on.
        path_kind: crate::windows::PathKind,
        /// What the folder check said.
        support: crate::windows::FolderRefusal,
    },
    /// Windows tier B can't fence another registered matter (`windows::admit`): one of its
    /// folders exists but can't take the deny entries that close it to this launch's agent, so
    /// the launch is refused rather than leave a readable neighbour. Names the matter by its id
    /// and the folder by its kind, never a path. Nothing was planned or written.
    #[error("the sandbox can't fence matter {matter}: {path_kind}: {support}")]
    NeighbourUnfenceable {
        /// The other matter.
        matter: clauseo_protocol::MatterId,
        /// Which of its folders.
        path_kind: crate::windows::PathKind,
        /// What the check said.
        support: crate::windows::FolderRefusal,
    },
    /// Windows tier B couldn't apply its permission entries (`windows::apply`): a write, the
    /// walk or the ledger failed, or the walk found a child that would beat an inherited deny.
    /// This launch's new entries were rolled back. Its message names the kind of path, never
    /// the path.
    #[error("the sandbox couldn't set the permissions of {path_kind}: {failure}")]
    AclApply {
        /// Which path: the matter, a sibling and so on.
        path_kind: crate::windows::PathKind,
        /// What failed.
        failure: crate::windows::ApplyFailure,
        /// The file or folder a walk refused, when it was one: for the running page to show the
        /// lawyer (a protected child, a broad allow, one the lawyer can't open). Never logged.
        at: Option<std::path::PathBuf>,
    },
    /// Windows tier B couldn't set aside a protected name that changed in a session, or the
    /// item the strict walk found the lawyer can't open, or couldn't read or write the record it
    /// checks against (`windows::set_aside`). The launch is refused, and the record stays as it
    /// was, so the next launch checks again. Its message names no path.
    #[error("the sandbox couldn't set aside a changed protected name: {failure}")]
    SetAside {
        /// What failed.
        failure: crate::windows::SetAsideFailure,
        /// The item that couldn't be read or moved, when it was one: for the running page to
        /// show the lawyer (one on another drive, one whose folder the lawyer can't delete
        /// from). Never logged.
        at: Option<std::path::PathBuf>,
    },
}

impl Error {
    /// The code a client sees. A refused policy stops the launch, so it is a failed launch, as
    /// a launch error is, and so are the proxy's errors and a folder tier B can't prepare (the
    /// caller falls back to the tier it can get); a failed probe is unexpected, so it is
    /// `Internal`.
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::Policy(_)
            | Error::ProxyConfig(_)
            | Error::ProxyStart(_)
            | Error::FolderUnsupported { .. }
            | Error::NeighbourUnfenceable { .. }
            | Error::AclApply { .. }
            | Error::SetAside { .. } => ErrorCode::AgentLaunchFailed,
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
        let folder = Error::FolderUnsupported {
            path_kind: crate::windows::PathKind::State,
            support: crate::windows::FolderRefusal::Support(crate::windows::FolderSupport::Network),
        };
        assert_eq!(folder.code(), ErrorCode::AgentLaunchFailed);
        assert_eq!(
            folder.to_string(),
            "the sandbox can't prepare the state folder: it is on a network drive"
        );
    }
}
