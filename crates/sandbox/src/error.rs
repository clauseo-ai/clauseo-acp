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
    /// Windows tier B couldn't remove the matter's clean-close record at launch
    /// (`windows::clean_close`): every launch removes it before the agent starts, so the launch
    /// is refused rather than run with it in place (S15 `PR3d`).
    #[error("the sandbox couldn't remove the record of the matter's last clean close ({0})")]
    CleanClose(io::ErrorKind),
    /// Windows tier B found a protected name in the matter (a `.git`, `.claude`, `.mcp.json`,
    /// `CLAUDE.md` or `AGENTS.md` at any depth, or another protected name at the root) that
    /// is a junction or symbolic link, so no entry can protect it (`windows::apply`, S15
    /// `PR4a2`). The launch's new entries were rolled back. The lawyer moves or deletes it.
    #[error(
        "{} is a shortcut, and Clauseo can't protect a shortcut. Move or delete it, then open \
         the matter again.",
        at.display()
    )]
    ProtectedLink {
        /// The item's path relative to the matter (`sub\AGENTS.md`), never an absolute path:
        /// for PR6 to show the lawyer.
        at: std::path::PathBuf,
    },
    /// Windows tier B can't filter this launch's network (`windows::wfp`, S15 PR7): set-up's
    /// WFP filters are missing, changed, unreadable or don't name the matter's pool user, the
    /// policy asks for what tier B never permits, or the launch's permit failed. The launch is
    /// refused, so no agent runs unfiltered.
    #[error("{cause}")]
    NetworkUnfiltered {
        /// Why.
        cause: crate::windows::wfp::NetworkGap,
    },
    /// Windows tier B refuses to launch for a cause only whoever looks after the computer can
    /// fix (D-B4): the set-up program's admin copy is missing, changed or in a folder others
    /// can change (`Blocked::AdminCopy*`, from `windows::clean_close::check_launch`), or gone
    /// after Windows security blocked it (`Blocked::ToolMissing`). Nothing was planned or
    /// written, and the caller shows the cause rather than run on a weaker tier in silence.
    #[error("the sandbox can't run: {0}")]
    Blocked(crate::Blocked),
    /// Windows tier B found an earlier session's unclean-session marker still awaiting a sweep
    /// of the pool users' scheduled tasks and BITS jobs, asked for one, and it didn't complete
    /// within 15 s, or the markers or the record's sequence couldn't be read
    /// (`windows::runner::unclean`, from `windows::clean_close::check_launch`). Nothing was
    /// planned or written; a later launch checks again. Its message names no path.
    #[error("the sandbox's clean-up after an earlier session isn't finished: {0}")]
    UncleanSession(String),
    /// The policy puts a Claude store or the browser socket folder on, inside or above a hidden
    /// path ([`PolicyBuilder::build`](crate::PolicyBuilder::build)): the profile allows their
    /// writes after the hidden paths, so they would write there. A Claude store inside a hidden
    /// path that is not another matter is allowed (Clauseo's login home is in the data dir).
    #[error(
        "sandbox policy refused: the {allowance} may not equal, hold or sit inside a hidden \
         path: {} and {}",
        path.display(),
        hidden.display()
    )]
    HiddenOverlap {
        /// Which allowance.
        allowance: crate::HiddenOverlapAllowance,
        /// Its folder, canonical.
        path: std::path::PathBuf,
        /// The hidden path it meets.
        hidden: std::path::PathBuf,
    },
}

impl Error {
    /// The [`Blocked`](crate::Blocked) cause's serde name for [`Error::Blocked`]
    /// (`Blocked::name`), `None` for every other error: path-free and without `Display`'s
    /// words, for the host to put in the protocol's `NotProtectedCause::Blocked`.
    pub fn cause(&self) -> Option<&'static str> {
        match self {
            Error::Blocked(blocked) => Some(blocked.name()),
            Error::Policy(_)
            | Error::Probe(_)
            | Error::Launch(_)
            | Error::ProxyConfig(_)
            | Error::ProxyStart(_)
            | Error::FolderUnsupported { .. }
            | Error::NeighbourUnfenceable { .. }
            | Error::AclApply { .. }
            | Error::SetAside { .. }
            | Error::CleanClose(_)
            | Error::NetworkUnfiltered { .. }
            | Error::ProtectedLink { .. }
            | Error::UncleanSession(_)
            | Error::HiddenOverlap { .. } => None,
        }
    }

    /// The code a client sees. A refused policy stops the launch, so it is a failed launch, as
    /// a launch error is, and so are the proxy's errors and a folder tier B can't prepare (the
    /// caller falls back to the tier it can get); a failed probe is unexpected, so it is
    /// `Internal`. Tier B's network set-up missing or stale is `sandbox.setup_required`, since
    /// running the set-up again fixes it; any other network refusal is a failed launch.
    /// A block only an administrator can lift is `SandboxBlocked`, not retryable. An earlier
    /// session's clean-up not yet finished is `SandboxCleaningUp`, retryable: a later launch
    /// checks again.
    pub fn code(&self) -> ErrorCode {
        match self {
            Error::NetworkUnfiltered { cause } if cause.needs_setup() => {
                ErrorCode::SandboxSetupRequired
            }
            Error::Policy(_)
            | Error::ProxyConfig(_)
            | Error::ProxyStart(_)
            | Error::FolderUnsupported { .. }
            | Error::NeighbourUnfenceable { .. }
            | Error::AclApply { .. }
            | Error::SetAside { .. }
            | Error::CleanClose(_)
            | Error::NetworkUnfiltered { .. }
            | Error::HiddenOverlap { .. } => ErrorCode::AgentLaunchFailed,
            Error::Blocked(_) => ErrorCode::SandboxBlocked,
            Error::UncleanSession(_) => ErrorCode::SandboxCleaningUp,
            Error::ProtectedLink { .. } => ErrorCode::MatterProtectedLink,
            Error::Probe(_) => ErrorCode::Internal,
            Error::Launch(error) => error.code(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every cause's name, listed: a new `Blocked` variant must be added to `Blocked::ALL` and
    /// here, and its name must be the one serde writes.
    #[test]
    fn every_blocked_cause_has_its_serde_name() {
        let names: Vec<&str> = crate::Blocked::ALL.iter().map(|b| b.name()).collect();
        assert_eq!(
            names,
            [
                "logon_right",
                "policy",
                "admin_copy_missing",
                "admin_copy_changed",
                "admin_copy_unsafe",
                "other_account",
                "tool_missing",
            ]
        );
        for blocked in crate::Blocked::ALL {
            let serde = serde_json::to_string(&blocked).unwrap();
            assert_eq!(serde, format!("\"{}\"", blocked.name()));
            assert_eq!(
                Error::Blocked(blocked).cause(),
                Some(blocked.name()),
                "{blocked:?}"
            );
        }
    }

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
        assert_eq!(folder.cause(), None);
        assert_eq!(launch.cause(), None);
        let changed = Error::Blocked(crate::Blocked::AdminCopyChanged);
        assert_eq!(changed.cause(), Some("admin_copy_changed"));
        assert_eq!(changed.code(), ErrorCode::SandboxBlocked);
        assert!(
            !changed.code().is_retryable(),
            "only an administrator can fix it"
        );
        assert!(
            changed.to_string().starts_with("the sandbox can't run: "),
            "{changed}"
        );
        let unclean = Error::UncleanSession("the sweep didn't finish within 15 s".to_owned());
        assert_eq!(unclean.code(), ErrorCode::SandboxCleaningUp);
        assert_eq!(unclean.code().as_str(), "sandbox.cleaning_up");
        assert!(unclean.code().is_retryable(), "a later launch checks again");
        assert_eq!(unclean.cause(), None);
    }
}
