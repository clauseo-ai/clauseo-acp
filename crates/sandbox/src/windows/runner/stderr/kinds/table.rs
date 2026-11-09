//! Every runner stderr kind's row: its stable name, the [`RunExit`] a refusal of it fails the run
//! with, its class and its fixed sentence. Names never change once given: the host maps them.

use super::super::super::{PASSWORD_LOST, RunExit, SECLOGON_DISABLED};
use super::super::PAIRING_LOST;
use super::KindClass::{self, Admin, Internal, Notice, Permanent, Setup, Transient};
use super::RunnerKind;

/// A kind's fixed parts: one row of [`TABLE`].
pub(in super::super) struct Row {
    pub(in super::super) kind: RunnerKind,
    pub(in super::super) name: &'static str,
    pub(in super::super) exit: Option<RunExit>,
    pub(in super::super) class: KindClass,
    pub(in super::super) sentence: &'static str,
}

/// Every kind's row, in the enum's order (`tests.rs` checks it): refusals first, in
/// `RunExit`'s order, then the notices.
pub(in super::super) const TABLE: [Row; 54] = [
    Row {
        kind: RunnerKind::Usage,
        name: "usage",
        exit: Some(RunExit::Usage),
        class: Internal,
        sentence: "the sandbox runner was started the wrong way",
    },
    Row {
        kind: RunnerKind::BadLaunch,
        name: "bad_launch",
        exit: Some(RunExit::BadLaunch),
        class: Internal,
        sentence: "the sandbox runner couldn't read or accept the launch it was given",
    },
    Row {
        kind: RunnerKind::PoolUserMissing,
        name: "pool_user_missing",
        exit: Some(RunExit::BadLaunch),
        class: Setup,
        sentence: "the matter's sandbox user isn't among those set up on this computer: run the \
         set-up again",
    },
    Row {
        kind: RunnerKind::NoSecret,
        name: "no_secret",
        exit: Some(RunExit::NoSecret),
        class: Setup,
        sentence: "the sandbox user's saved password is missing or can't be read: run the set-up \
         again",
    },
    Row {
        kind: RunnerKind::SecondaryLogonDisabled,
        name: "secondary_logon_disabled",
        exit: Some(RunExit::SecondaryLogon),
        class: Admin,
        sentence: SECLOGON_DISABLED,
    },
    Row {
        kind: RunnerKind::LogonRightMissing,
        name: "logon_right_missing",
        exit: Some(RunExit::LogonRight),
        class: Admin,
        sentence: "Windows doesn't allow the sandbox users to log on locally: IT can allow it",
    },
    Row {
        kind: RunnerKind::PasswordRefused,
        name: "password_refused",
        exit: Some(RunExit::Password),
        class: Setup,
        sentence: PASSWORD_LOST,
    },
    Row {
        kind: RunnerKind::Logon,
        name: "logon",
        exit: Some(RunExit::Logon),
        class: Transient,
        sentence: "the sandbox user couldn't be logged on",
    },
    Row {
        kind: RunnerKind::CodexReuse,
        name: "codex_reuse",
        exit: Some(RunExit::CodexReuse),
        class: Permanent,
        sentence: "the launch would use your own Codex folder, which the sandbox never shares",
    },
    Row {
        kind: RunnerKind::Pipe,
        name: "pipe",
        exit: Some(RunExit::Pipe),
        class: Transient,
        sentence: "the sandbox runner's private connection to its other half failed",
    },
    Row {
        kind: RunnerKind::PipeIntruder,
        name: "pipe_intruder",
        exit: Some(RunExit::Pipe),
        class: Admin,
        sentence: "another program connected to the sandbox runner's private connection",
    },
    Row {
        kind: RunnerKind::Job,
        name: "job",
        exit: Some(RunExit::Job),
        class: Transient,
        sentence: "the sandbox couldn't hold the agent's processes together, so the agent didn't \
         start",
    },
    Row {
        kind: RunnerKind::Token,
        name: "token",
        exit: Some(RunExit::Token),
        class: Transient,
        sentence: "the sandbox couldn't make the agent's restricted access, so the agent didn't \
         start",
    },
    Row {
        kind: RunnerKind::Spawn,
        name: "spawn",
        exit: Some(RunExit::Spawn),
        class: Transient,
        sentence: "the agent couldn't be started inside the sandbox",
    },
    Row {
        kind: RunnerKind::MatterUnwatchable,
        name: "matter_unwatchable",
        exit: Some(RunExit::Spawn),
        class: Transient,
        sentence: "Clauseo can't watch the matter's folder for changes, so it didn't start the \
         session",
    },
    Row {
        kind: RunnerKind::Internal,
        name: "internal",
        exit: Some(RunExit::Spawn),
        class: Internal,
        sentence: "the sandbox runner failed in a way it can't name",
    },
    Row {
        kind: RunnerKind::AgentReserved,
        name: "agent_reserved",
        exit: Some(RunExit::AgentReserved),
        class: Internal,
        sentence: "the agent exited with a code kept for the sandbox runner's own failures",
    },
    Row {
        kind: RunnerKind::EndedBeforeStart,
        name: "ended_before_start",
        exit: Some(RunExit::AgentReserved),
        class: Transient,
        sentence: "the session ended before its agent started; the agent never ran",
    },
    Row {
        kind: RunnerKind::Sweep,
        name: "sweep",
        exit: Some(RunExit::Sweep),
        class: Transient,
        sentence: "the sandbox couldn't clear the sandbox user's earlier processes, so the agent \
         didn't start",
    },
    Row {
        kind: RunnerKind::DllSearch,
        name: "dll_search",
        exit: Some(RunExit::DllSearch),
        class: Internal,
        sentence: "the sandbox runner couldn't keep Windows to its own folder for the libraries it \
         loads",
    },
    Row {
        kind: RunnerKind::Escapee,
        name: "escapee",
        exit: Some(RunExit::Escapee),
        class: Transient,
        sentence: "a process of the sandbox user's outlived every sweep of this session",
    },
    Row {
        kind: RunnerKind::MarkerUnwritable,
        name: "marker_unwritable",
        exit: Some(RunExit::MarkerUnwritable),
        class: Transient,
        sentence: "the session's marker couldn't be written, so the session didn't start",
    },
    Row {
        kind: RunnerKind::InstallRecordUnreadable,
        name: "install_record_unreadable",
        exit: Some(RunExit::MarkerUnwritable),
        class: Setup,
        sentence: "the sandbox's install record can't be read, so the session didn't start: run \
         the set-up again",
    },
    Row {
        kind: RunnerKind::AwaitingStop,
        name: "awaiting_stop",
        exit: Some(RunExit::AwaitingStop),
        class: Transient,
        sentence: "an earlier session of this sandbox user awaits the sandbox's stop",
    },
    Row {
        kind: RunnerKind::AwaitingSweep,
        name: "awaiting_sweep",
        exit: Some(RunExit::AwaitingSweep),
        class: Transient,
        sentence: "an earlier session of this sandbox user awaits the sandbox's sweep",
    },
    Row {
        kind: RunnerKind::NotWindows,
        name: "not_windows",
        exit: None,
        class: Permanent,
        sentence: "it runs agents in Clauseo's sandbox on Windows only",
    },
    Row {
        kind: RunnerKind::GitPairingLost,
        name: "git_pairing_lost",
        exit: None,
        class: Notice,
        sentence: PAIRING_LOST,
    },
    Row {
        kind: RunnerKind::GitWatchBroken,
        name: "git_watch_broken",
        exit: None,
        class: Notice,
        sentence: "git watch: the watch failed; ending the session",
    },
    Row {
        kind: RunnerKind::GitWatchOverflow,
        name: "git_watch_overflow",
        exit: None,
        class: Notice,
        sentence: "git watch: too many changes at once; rescanning the folder",
    },
    Row {
        kind: RunnerKind::GitItemsLeft,
        name: "git_items_left",
        exit: None,
        class: Notice,
        sentence: "git watch: items not set aside while the agent's processes may still run; the \
         check after the session sets them aside",
    },
    Row {
        kind: RunnerKind::GitItemStuck,
        name: "git_item_stuck",
        exit: None,
        class: Notice,
        sentence: "git watch: an item couldn't be set aside; trying again once the session ended",
    },
    Row {
        kind: RunnerKind::GitItemsStillStuck,
        name: "git_items_still_stuck",
        exit: None,
        class: Notice,
        sentence: "git watch: items still couldn't be set aside once the session ended; the check \
         after the session tries again",
    },
    Row {
        kind: RunnerKind::GitWatchBusyAtEnd,
        name: "git_watch_busy_at_end",
        exit: None,
        class: Notice,
        sentence: "git watch: still busy at the session's end; the check after the session covers \
         it",
    },
    Row {
        kind: RunnerKind::JobNotEmptied,
        name: "job_not_emptied",
        exit: None,
        class: Notice,
        sentence: "the session's job didn't empty; the sandbox sweep task is requested all the same",
    },
    Row {
        kind: RunnerKind::SweepTaskUnrequested,
        name: "sweep_task_unrequested",
        exit: None,
        class: Notice,
        sentence: "the sandbox sweep task couldn't be requested",
    },
    Row {
        kind: RunnerKind::StopTaskUnrequested,
        name: "stop_task_unrequested",
        exit: None,
        class: Notice,
        sentence: "the sandbox stop task couldn't be requested",
    },
    Row {
        kind: RunnerKind::SequenceUnreadable,
        name: "sequence_unreadable",
        exit: None,
        class: Notice,
        sentence: "a sandbox task's number can't be read; the session marker stays open",
    },
    Row {
        kind: RunnerKind::MarkerUnsettled,
        name: "marker_unsettled",
        exit: None,
        class: Notice,
        sentence: "the session marker couldn't be written; it stays open",
    },
    Row {
        kind: RunnerKind::EndPanicked,
        name: "end_panicked",
        exit: None,
        class: Notice,
        sentence: "the session's end panicked on its own thread; the exit ends it again",
    },
    Row {
        kind: RunnerKind::StdinWriterFailed,
        name: "stdin_writer_failed",
        exit: None,
        class: Notice,
        sentence: "the agent's stdin writer didn't start",
    },
    Row {
        kind: RunnerKind::MarkerUnremoved,
        name: "marker_unremoved",
        exit: None,
        class: Notice,
        sentence: "an earlier session's marker is clean but couldn't be removed",
    },
    Row {
        kind: RunnerKind::CleanupUnlisted,
        name: "cleanup_unlisted",
        exit: None,
        class: Notice,
        sentence: "cleanup: a kind of the sandbox user's leftovers can't be listed",
    },
    Row {
        kind: RunnerKind::CleanupRemoved,
        name: "cleanup_removed",
        exit: None,
        class: Notice,
        sentence: "cleanup: removed leftovers of the sandbox user's",
    },
    Row {
        kind: RunnerKind::CleanupIncomplete,
        name: "cleanup_incomplete",
        exit: None,
        class: Notice,
        sentence: "cleanup: some of the sandbox user's leftovers couldn't be removed",
    },
    Row {
        kind: RunnerKind::SweepKilled,
        name: "sweep_killed",
        exit: None,
        class: Notice,
        sentence: "sweep: killed a process of the sandbox user's",
    },
    Row {
        kind: RunnerKind::SweepExitedAfterRefusal,
        name: "sweep_exited_after_refusal",
        exit: None,
        class: Notice,
        sentence: "sweep: a process of the sandbox user's exited by itself after its kill was \
         refused",
    },
    Row {
        kind: RunnerKind::SweepFailed,
        name: "sweep_failed",
        exit: None,
        class: Notice,
        sentence: "sweep: a process of the sandbox user's couldn't be checked or ended",
    },
    Row {
        kind: RunnerKind::SweepEscapee,
        name: "sweep_escapee",
        exit: None,
        class: Notice,
        sentence: "sweep: processes of the sandbox user's can't be ended",
    },
    Row {
        kind: RunnerKind::LaunchFileKept,
        name: "launch_file_kept",
        exit: None,
        class: Notice,
        sentence: "the launch file couldn't be removed and stays",
    },
    Row {
        kind: RunnerKind::FirmCaUnmerged,
        name: "firm_ca_unmerged",
        exit: None,
        class: Notice,
        sentence: "the firm CA wasn't merged; the machine bundle stays",
    },
    Row {
        kind: RunnerKind::FirmCaFileSkipped,
        name: "firm_ca_file_skipped",
        exit: None,
        class: Notice,
        sentence: "a firm CA file was left out",
    },
    Row {
        kind: RunnerKind::FirmCaEntriesSkipped,
        name: "firm_ca_entries_skipped",
        exit: None,
        class: Notice,
        sentence: "firm CA entries were left out: unreadable, disallowed or not a CA",
    },
    Row {
        kind: RunnerKind::AgentWindowsStatus,
        name: "agent_windows_status",
        exit: None,
        class: Notice,
        sentence: "the agent ended at a Windows status",
    },
    Row {
        kind: RunnerKind::InnerWindowsStatus,
        name: "inner_windows_status",
        exit: None,
        class: Notice,
        sentence: "the inner runner ended at a Windows status",
    },
];
