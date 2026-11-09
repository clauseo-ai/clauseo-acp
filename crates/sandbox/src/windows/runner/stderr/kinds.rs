//! The runner's stderr kinds (`stderr.rs`): each kind's stable name, the [`RunExit`] a refusal of
//! it fails the run with, what the lawyer can do about it and its fixed sentence, in one table
//! (`table.rs`), whose rows are in the enum's order. 0049's table lists the same.

use serde::{Deserialize, Serialize};

use super::super::RunExit;
use table::{Row, TABLE};

mod table;

/// What the lawyer can do about a kind: the host's classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KindClass {
    /// Running Clauseo's set-up again fixes it.
    Setup,
    /// An administrator (IT) has to change something on the computer.
    Admin,
    /// It passes: a later launch may go ahead.
    Transient,
    /// Nothing on this computer, as it is, fixes it.
    Permanent,
    /// A fault of Clauseo's own: trying again may help, and it is worth reporting.
    Internal,
    /// A notice: nothing was refused.
    Notice,
}

impl KindClass {
    /// Its name in 0049's table.
    pub fn name(self) -> &'static str {
        match self {
            KindClass::Setup => "setup",
            KindClass::Admin => "admin",
            KindClass::Transient => "transient",
            KindClass::Permanent => "permanent",
            KindClass::Internal => "internal",
            KindClass::Notice => "notice",
        }
    }
}

/// Every line's kind. Closed, and its names stable: the host maps them to its own error codes.
/// It crosses `ctl` by the same name (`InnerReport::Log`).
/// A refusal's kind names the [`RunExit`] the run fails with ([`RunnerKind::exit`]); a finer kind
/// shares its code with the code's own kind only where the lawyer acts differently.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerKind {
    // Refusals, in `RunExit`'s order.
    /// [`RunExit::Usage`].
    Usage,
    /// [`RunExit::BadLaunch`].
    BadLaunch,
    /// [`RunExit::BadLaunch`]: `secrets.bin` holds no such pool user.
    PoolUserMissing,
    /// [`RunExit::NoSecret`].
    NoSecret,
    /// [`RunExit::SecondaryLogon`].
    SecondaryLogonDisabled,
    /// [`RunExit::LogonRight`].
    LogonRightMissing,
    /// [`RunExit::Password`].
    PasswordRefused,
    /// [`RunExit::Logon`].
    Logon,
    /// [`RunExit::CodexReuse`].
    CodexReuse,
    /// [`RunExit::Pipe`].
    Pipe,
    /// [`RunExit::Pipe`]: another process connected a runner pipe.
    PipeIntruder,
    /// [`RunExit::Job`].
    Job,
    /// [`RunExit::Token`].
    Token,
    /// [`RunExit::Spawn`].
    Spawn,
    /// [`RunExit::Spawn`]: the git watch couldn't be armed on the matter's folder.
    MatterUnwatchable,
    /// [`RunExit::Spawn`]: the runner's own event or thread couldn't be made, or the inner
    /// runner reported a code that isn't one of [`RunExit`]'s.
    Internal,
    /// [`RunExit::AgentReserved`].
    AgentReserved,
    /// [`RunExit::AgentReserved`]: the session ended (a git watch's end, a cancel) before the
    /// agent started.
    EndedBeforeStart,
    /// [`RunExit::Sweep`].
    Sweep,
    /// [`RunExit::DllSearch`].
    DllSearch,
    /// [`RunExit::Escapee`].
    Escapee,
    /// [`RunExit::MarkerUnwritable`].
    MarkerUnwritable,
    /// [`RunExit::MarkerUnwritable`]: the install's record can't be read for the marker.
    InstallRecordUnreadable,
    /// [`RunExit::AwaitingStop`].
    AwaitingStop,
    /// [`RunExit::AwaitingSweep`].
    AwaitingSweep,
    /// The runner started off Windows: exit code 2, no [`RunExit`].
    NotWindows,
    // Notices.
    /// The git watch's pairing is lost for the session (`git_watch::PAIRING_LOST`).
    GitPairingLost,
    /// The git watch failed and ends the session.
    GitWatchBroken,
    /// Too many changes at once: the git watch rescans.
    GitWatchOverflow,
    /// Items the git watch left while the agent's processes may run.
    GitItemsLeft,
    /// An item the git watch couldn't set aside.
    GitItemStuck,
    /// Items still not set aside once the session ended.
    GitItemsStillStuck,
    /// A git watch still busy at the session's end.
    GitWatchBusyAtEnd,
    /// The session's job didn't empty.
    JobNotEmptied,
    /// The `SYSTEM` sweep task couldn't be requested.
    SweepTaskUnrequested,
    /// The `SYSTEM` stop task couldn't be requested.
    StopTaskUnrequested,
    /// A task's sequence number (`SweepNext`, `StopNext`) can't be read.
    SequenceUnreadable,
    /// The session's marker couldn't be written at the end.
    MarkerUnsettled,
    /// The session's end panicked on its own thread.
    EndPanicked,
    /// The agent's stdin writer didn't start.
    StdinWriterFailed,
    /// An earlier session's clean marker couldn't be removed.
    MarkerUnremoved,
    /// The inner's cleanup couldn't list a kind of leftover.
    CleanupUnlisted,
    /// The inner's cleanup removed leftovers.
    CleanupRemoved,
    /// The inner's cleanup couldn't remove some leftovers.
    CleanupIncomplete,
    /// A sweep killed a process of the pool user's.
    SweepKilled,
    /// A process whose kill was refused exited by itself.
    SweepExitedAfterRefusal,
    /// A sweep failed (mid-session or at the end).
    SweepFailed,
    /// A sweep found processes of the pool user's it can't end.
    SweepEscapee,
    /// The launch file couldn't be removed.
    LaunchFileKept,
    /// The firm CA files weren't merged.
    FirmCaUnmerged,
    /// A firm CA file was left out.
    FirmCaFileSkipped,
    /// Some firm CA entries were left out.
    FirmCaEntriesSkipped,
    /// The agent ended at a Windows status.
    AgentWindowsStatus,
    /// The inner runner ended at a Windows status.
    InnerWindowsStatus,
}

impl RunnerKind {
    /// Every kind, refusals first.
    pub fn all() -> impl Iterator<Item = RunnerKind> {
        TABLE.iter().map(|row| row.kind)
    }

    /// Its row in `table.rs`.
    fn row(self) -> &'static Row {
        let at = self as usize;
        TABLE
            .get(at)
            .filter(|row| row.kind == self)
            .unwrap_or_else(|| {
                TABLE
                    .iter()
                    .find(|row| row.kind == self)
                    .unwrap_or(&TABLE[0])
            })
    }

    /// Its stable `snake_case` name.
    pub fn name(self) -> &'static str {
        self.row().name
    }

    /// Its fixed sentence: plain English, with no path, name or value.
    pub fn sentence(self) -> &'static str {
        self.row().sentence
    }

    /// What the lawyer can do about it.
    pub fn class(self) -> KindClass {
        self.row().class
    }

    /// The exit a refusal of this kind fails the run with; `None` for a notice, and for
    /// [`RunnerKind::NotWindows`] (exit code 2).
    pub fn exit(self) -> Option<RunExit> {
        self.row().exit
    }

    /// Whether it is a notice's kind, which [`notice`] writes; else [`refuse`] does.
    pub fn is_notice(self) -> bool {
        self.class() == KindClass::Notice
    }

    /// The kind an `exit` is said with when nothing finer is known: the code's own kind. `None`
    /// for [`RunExit::PoolStop`], which is no longer produced.
    pub fn for_exit(exit: RunExit) -> Option<RunnerKind> {
        Some(match exit {
            RunExit::Usage => RunnerKind::Usage,
            RunExit::BadLaunch => RunnerKind::BadLaunch,
            RunExit::NoSecret => RunnerKind::NoSecret,
            RunExit::SecondaryLogon => RunnerKind::SecondaryLogonDisabled,
            RunExit::LogonRight => RunnerKind::LogonRightMissing,
            RunExit::Password => RunnerKind::PasswordRefused,
            RunExit::Logon => RunnerKind::Logon,
            RunExit::CodexReuse => RunnerKind::CodexReuse,
            RunExit::Pipe => RunnerKind::Pipe,
            RunExit::Job => RunnerKind::Job,
            RunExit::Token => RunnerKind::Token,
            RunExit::Spawn => RunnerKind::Spawn,
            RunExit::AgentReserved => RunnerKind::AgentReserved,
            RunExit::Sweep => RunnerKind::Sweep,
            RunExit::DllSearch => RunnerKind::DllSearch,
            RunExit::Escapee => RunnerKind::Escapee,
            RunExit::PoolStop => return None,
            RunExit::MarkerUnwritable => RunnerKind::MarkerUnwritable,
            RunExit::AwaitingStop => RunnerKind::AwaitingStop,
            RunExit::AwaitingSweep => RunnerKind::AwaitingSweep,
        })
    }

    /// The kind an exit code is said with: [`RunnerKind::for_exit`] of the [`RunExit`] it is,
    /// else [`RunnerKind::Internal`].
    pub fn for_code(code: u32) -> RunnerKind {
        RunExit::from_code(code)
            .and_then(RunnerKind::for_exit)
            .unwrap_or(RunnerKind::Internal)
    }
}
