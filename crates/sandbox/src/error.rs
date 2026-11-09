//! Every failure of this crate, and the code each one reaches a client as.

use std::io;

use clauseo_protocol::ErrorCode;

/// Why a policy was refused, a look at the machine failed, or a jailed agent did not start.
///
/// `#[non_exhaustive]`: variants are appended (never inserted or renumbered), so a match
/// outside this crate keeps a wildcard arm. Inside it, [`Error::cause`], [`Error::code`] and
/// [`Error::kind`] name every variant, so a new one doesn't build until it has its code.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
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
    /// Windows tier B has no free account for this matter (`windows::pool::assign`'s
    /// `Assign::PoolFull`): every pool user is held back. Counts only, never an account: the
    /// host shows `NotProtectedCause::PoolFull` from them, and the matter has no sandbox until
    /// one is free (PR6).
    #[error("the sandbox has no free account: {cause}")]
    PoolFull {
        /// How many of the pool's users are held back, and why.
        cause: crate::windows::pool::PoolFullCause,
    },
    /// Windows tier B was asked to launch on a reservation that `prepare` hasn't finished for
    /// (`windows::launch`): the trees are never walked inline. Retryable: the host reserves and
    /// prepares again.
    #[error("the sandbox hasn't prepared this matter yet")]
    NotPrepared,
    /// Windows tier B's preparation of the matter was cancelled (`Preparing::cancel`, on matter
    /// close or app quit). What it had done for the session was undone; the permission entries
    /// already written stay in the ledger, so the next `prepare` continues from them.
    #[error("the sandbox's preparation of this matter was cancelled")]
    Cancelled,
    /// Windows tier B runs one live agent per matter (`windows::launch`'s `reserve`): another
    /// session of the matter holds its account, from its reservation until its after-session
    /// pass and its end sweeper have finished, or `assign` found another session open. A
    /// refusal, not a cause for another tier: **the host retries** within its 180 s bound,
    /// showing that it is preparing (it stops the other agent or waits). Only `reserve` gives it.
    #[error("Another agent is still working in this matter. Try again once it has finished.")]
    MatterBusy,
    /// Windows tier B's `launch` or `prepare` was given a reservation for another matter or
    /// session, or one these launchers didn't hand out or no longer hold (used, released, or
    /// cleared at start-up). A bug in the caller: nothing was run or granted.
    #[error("the sandbox's reservation is for another matter or session, or no longer held")]
    WrongReservation,
    /// Windows tier B found a process of the matter's pool user still running outside this
    /// launch (`windows::launch`). **Final, never retried:** `prepare` gives it only after the
    /// stop task, the pool user's sweep and a fresh look, again and again, until its 180 s
    /// bound; `launch` gives it when one started after the preparation. A plain launch failure.
    /// Never from `reserve`, and the end sweeper never counts.
    #[error(
        "A program from an earlier agent run kept running in the Safe workspace, so the agent \
         couldn't start."
    )]
    LeftoverProcess {
        /// How many there were.
        count: usize,
    },
    /// Windows tier B's launchers couldn't start: another set of launchers, in this Clauseo or
    /// another, holds the sandbox folder's lock (`tier-b-launchers.lock`). Refused at once,
    /// never waited for: two would clear each other's live marks. Fixed by closing the other
    /// Clauseo.
    #[error("Another Clauseo is already using the Safe workspace on this computer.")]
    LaunchersHeld,
    /// Windows tier B's work for this matter (a walk, its undo, or a session's end) is still in
    /// one step past the 180 s bound and its grace (`windows::launch`'s `STUCK_GRACE`). Nothing
    /// is undone from another thread (the step could still grant once it returns), so the
    /// matter refuses every launch until a restart. Until Clauseo stops pool-user processes at
    /// its start (0049's C5 list), the words say to restart the computer. **Not retryable.**
    #[error(
        "A step of the Safe workspace stopped answering for this matter. Restart the computer \
         to continue with this matter."
    )]
    MatterStuck,
    /// Windows tier B's preparation of the matter made no counted progress for 60 s
    /// (`windows::launch`'s `PREPARE_STALL`), or ran past its 20 min cap (`WORK_CEILING`).
    /// It was cancelled and what it had done for the session undone. A failed launch.
    #[error("Preparing the matter stopped making progress or took too long.")]
    PrepareStalled,
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
            | Error::PoolFull { .. }
            | Error::NotPrepared
            | Error::Cancelled
            | Error::MatterBusy
            | Error::WrongReservation
            | Error::LeftoverProcess { .. }
            | Error::LaunchersHeld
            | Error::MatterStuck
            | Error::PrepareStalled => None,
        }
    }

    /// What kind of failure it is, in words for the lawyer's message and the log: never a path,
    /// an account or a matter's name. Names every variant, so the host can use it in place of a
    /// match of its own, which `#[non_exhaustive]` would make end in a wildcard.
    pub fn kind(&self) -> &'static str {
        match self {
            Error::Policy(_) => "its rules for this matter were refused",
            Error::Probe(_) => "a look at the matter failed",
            Error::Launch(_) => "the jailed program did not start",
            Error::ProxyConfig(_) | Error::ProxyStart(_) => "the network filter did not start",
            Error::FolderUnsupported { .. } | Error::NeighbourUnfenceable { .. } => {
                "a folder can't be protected"
            }
            Error::AclApply { .. } => "the folder permissions couldn't be set",
            Error::SetAside { .. } => "a changed protected item couldn't be set aside",
            Error::CleanClose(_) => "the record of the matter's last session couldn't be removed",
            Error::ProtectedLink { .. } => "a protected item in the matter is a shortcut",
            Error::NetworkUnfiltered { .. } => "the agent's network couldn't be filtered",
            Error::Blocked(_) => "a setting on this computer stops the Safe workspace",
            Error::UncleanSession(_) => "the clean-up after an earlier session isn't finished",
            Error::PoolFull { .. } => "all of the Safe workspace's accounts are in use",
            Error::NotPrepared => "the matter isn't prepared yet",
            Error::Cancelled => "preparing the matter was cancelled",
            Error::MatterBusy => "another agent is still working in this matter",
            Error::WrongReservation => "the launch's reservation didn't match",
            Error::LeftoverProcess { .. } => "a program from an earlier agent run is still running",
            Error::LaunchersHeld => "another Clauseo is using the Safe workspace",
            Error::MatterStuck => "a step of the Safe workspace stopped answering",
            Error::PrepareStalled => {
                "preparing the matter stopped making progress or took too long"
            }
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
    ///
    /// Tier B's launcher (PR6): a full pool, a matter not prepared yet, a cancelled preparation,
    /// a busy matter, a leftover process, a stalled preparation and launchers another Clauseo
    /// holds are failed launches (retryable; the host decides what follows from the variant, not
    /// the code); a reservation that doesn't match is a bug, `Internal`, and so is a matter
    /// whose step hung past its limit (`MatterStuck`, not retryable: Clauseo must restart).
    /// Every one refuses the launch.
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
            | Error::PoolFull { .. }
            | Error::NotPrepared
            | Error::Cancelled
            | Error::MatterBusy
            | Error::LeftoverProcess { .. }
            | Error::LaunchersHeld
            | Error::PrepareStalled => ErrorCode::AgentLaunchFailed,
            Error::Blocked(_) => ErrorCode::SandboxBlocked,
            Error::UncleanSession(_) => ErrorCode::SandboxCleaningUp,
            Error::ProtectedLink { .. } => ErrorCode::MatterProtectedLink,
            Error::Probe(_) | Error::WrongReservation | Error::MatterStuck => ErrorCode::Internal,
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
                "loose_task",
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

    /// Tier B's launcher errors (PR6): each refuses the launch, with no cause for the protocol's
    /// `Blocked`, a kind of its own, and words with no path or account.
    #[test]
    fn launcher_errors_refuse_with_their_codes() {
        let full = Error::PoolFull {
            cause: crate::windows::pool::PoolFullCause {
                size: 8,
                open: 3,
                held: 2,
                owed: 2,
                set_aside: 1,
                needs_reset: 0,
            },
        };
        let cases = [
            (
                full,
                ErrorCode::AgentLaunchFailed,
                "all of the Safe workspace's accounts",
            ),
            (
                Error::NotPrepared,
                ErrorCode::AgentLaunchFailed,
                "isn't prepared",
            ),
            (Error::Cancelled, ErrorCode::AgentLaunchFailed, "cancelled"),
            (
                Error::MatterBusy,
                ErrorCode::AgentLaunchFailed,
                "another agent",
            ),
            (Error::WrongReservation, ErrorCode::Internal, "reservation"),
            (
                Error::LeftoverProcess { count: 2 },
                ErrorCode::AgentLaunchFailed,
                "earlier agent run",
            ),
            (
                Error::LaunchersHeld,
                ErrorCode::AgentLaunchFailed,
                "another Clauseo",
            ),
            (Error::MatterStuck, ErrorCode::Internal, "stopped answering"),
            (
                Error::PrepareStalled,
                ErrorCode::AgentLaunchFailed,
                "stopped making progress",
            ),
        ];
        let mut kinds = Vec::new();
        for (error, code, kind) in cases {
            assert_eq!(error.code(), code, "{error:?}");
            assert_eq!(error.cause(), None, "{error:?}");
            assert!(error.kind().contains(kind), "{error:?}: {}", error.kind());
            assert!(!error.to_string().contains('\\'), "no path: {error}");
            kinds.push(error.kind());
        }
        kinds.dedup();
        assert_eq!(kinds.len(), 9, "each has a kind of its own");
        assert!(
            !Error::MatterStuck.code().is_retryable(),
            "Clauseo must restart"
        );
        assert!(
            Error::MatterStuck
                .to_string()
                .contains("Restart the computer")
        );
        assert!(
            Error::NotPrepared.code().is_retryable(),
            "the host prepares again"
        );
        assert_eq!(
            Error::MatterBusy.to_string(),
            "Another agent is still working in this matter. Try again once it has finished."
        );
    }

    /// The loose-task block (0049's `loose_task`) is a block like the others: `SandboxBlocked`,
    /// with its serde name as the cause.
    #[test]
    fn loose_task_is_a_block() {
        let loose = Error::Blocked(crate::Blocked::LooseTask);
        assert_eq!(loose.code(), ErrorCode::SandboxBlocked);
        assert_eq!(loose.cause(), Some("loose_task"));
        assert_eq!(
            loose.kind(),
            "a setting on this computer stops the Safe workspace"
        );
    }
}
