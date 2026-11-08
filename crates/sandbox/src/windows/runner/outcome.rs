//! The outer runner's exit code, decoded for the host (PR6): the agent's own code, a runner
//! failure ([`RunExit`]), the host's own end of the outer ([`HOST_ENDED`]), or a code no runner
//! of this version gives. [`RunOutcome::meaning`] says what the host does with each.
//!
//! TODO(PR6, #717 and #757): map these `RunExit` codes in [`RunOutcome::meaning`], each as its
//! own `Meaning` that fails the launch with its cause:
//! - `RunExit::AwaitingStop` (#717): the stop task hasn't finished with the pool user;
//! - `RunExit::AwaitingSweep` (#717): a sweep of the pool user's is still running;
//! - `RunExit::LeftoverTask` (#757, not merged yet): the last end sweep left a task.
//!
//! Until then #717's two decode as [`RunOutcome::Runner`] and fail this launch
//! ([`Meaning::LaunchFailed`]); #757's decodes as [`RunOutcome::Unknown`] (in the runner's
//! range, not a code this build knows), which fails the launch too.

use super::{RUN_EXIT_BASE, RunExit};
use crate::NoSandbox;

/// The exit code the host's own job end gives the outer: `TERMINATED` in
/// `crates/process/src/os/windows.rs:54` (private there). What a cancel whose agent didn't exit
/// within the host's grace gives (`end.rs`). An agent that exits with this code itself reads as
/// [`RunOutcome::HostEnded`] too; the host only ever sees it after it asked for the end.
pub const HOST_ENDED: u32 = 0xC1A0;

/// The outer runner's exit, decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutcome {
    /// The agent ran and exited with this code, which the outer passed on
    /// ([`agent_code`](super::agent_code)).
    Agent(u32),
    /// The runner failed, or a sweep found an escapee, with this code.
    Runner(RunExit),
    /// The host's own shutdown ladder ended the outer ([`HOST_ENDED`]).
    HostEnded,
    /// No exit code, or one in the runner's range that this build doesn't know (a newer
    /// runner's).
    Unknown,
}

/// What the host does with a [`RunOutcome`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meaning {
    /// The agent's own exit: the host's usual handling of an agent that exited.
    AgentExited(u32),
    /// The host ended the run itself (a cancel): nothing more to report.
    HostEnded,
    /// A sweep found a process of the pool user's that no sweep could end
    /// ([`RunExit::Escapee`]): the host's "left the Safe workspace" stop (R17 PR-E). The outer
    /// has asked the stop task itself. It wins over the agent's own code, even 0.
    LeftSafeWorkspace,
    /// A set-up failure ([`RunExit::no_sandbox`]): the host detects the tier again with this
    /// cause, as running the set-up again or IT fixes it.
    Redetect(NoSandbox),
    /// The runner failed this launch only, with this code (`None`: [`RunOutcome::Unknown`]):
    /// `agent.launch_failed`.
    LaunchFailed(Option<RunExit>),
}

impl RunOutcome {
    /// Decodes the outer's exit status: `code` is `ExitStatus::code()`, the Windows exit code
    /// as an `i32`'s bits.
    pub fn from_exit(code: Option<i32>) -> RunOutcome {
        let Some(code) = code else {
            return RunOutcome::Unknown;
        };
        let code = u32::from_ne_bytes(code.to_ne_bytes());
        if code == HOST_ENDED {
            RunOutcome::HostEnded
        } else if let Some(exit) = RunExit::from_code(code) {
            RunOutcome::Runner(exit)
        } else if (RUN_EXIT_BASE..=RUN_EXIT_BASE + 0xff).contains(&code) {
            RunOutcome::Unknown
        } else {
            RunOutcome::Agent(code)
        }
    }

    /// What the host does with it: see [`Meaning`].
    pub fn meaning(self) -> Meaning {
        match self {
            RunOutcome::Agent(code) => Meaning::AgentExited(code),
            RunOutcome::HostEnded => Meaning::HostEnded,
            RunOutcome::Runner(RunExit::Escapee) => Meaning::LeftSafeWorkspace,
            RunOutcome::Runner(exit) => match exit.no_sandbox() {
                Some(cause) => Meaning::Redetect(cause),
                None => Meaning::LaunchFailed(Some(exit)),
            },
            RunOutcome::Unknown => Meaning::LaunchFailed(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exit(code: u32) -> Option<i32> {
        i32::try_from(code)
            .ok()
            .or(Some(i32::from_ne_bytes(code.to_ne_bytes())))
    }

    #[test]
    fn every_runner_code_decodes_to_its_exit() {
        for each in RunExit::ALL {
            assert_eq!(
                RunOutcome::from_exit(exit(each.code())),
                RunOutcome::Runner(each)
            );
        }
    }

    #[test]
    fn agents_codes_pass_through() {
        for code in [
            0,
            1,
            3,
            0xC000_0005,
            RUN_EXIT_BASE - 1,
            RUN_EXIT_BASE + 0x100,
        ] {
            let outcome = RunOutcome::from_exit(exit(code));
            assert_eq!(outcome, RunOutcome::Agent(code), "{code:#x}");
            assert_eq!(outcome.meaning(), Meaning::AgentExited(code));
        }
        // An agent's code in the runner's range comes through as `AgentReserved`.
        let reserved = super::super::agent_code(RUN_EXIT_BASE + 3);
        assert_eq!(
            RunOutcome::from_exit(exit(reserved)),
            RunOutcome::Runner(RunExit::AgentReserved)
        );
    }

    #[test]
    fn host_ended_and_unknown() {
        assert_eq!(HOST_ENDED, 0xC1A0);
        assert_eq!(
            RunOutcome::from_exit(exit(HOST_ENDED)),
            RunOutcome::HostEnded
        );
        assert_eq!(RunOutcome::HostEnded.meaning(), Meaning::HostEnded);
        assert_eq!(RunOutcome::from_exit(None), RunOutcome::Unknown);
        // A newer runner's code: in the range, past every exit this build knows.
        let newer = RUN_EXIT_BASE + u32::try_from(RunExit::ALL.len()).unwrap();
        assert_eq!(RunOutcome::from_exit(exit(newer)), RunOutcome::Unknown);
        assert_eq!(
            RunOutcome::from_exit(exit(RUN_EXIT_BASE + 0xff)),
            RunOutcome::Unknown
        );
        assert_eq!(RunOutcome::Unknown.meaning(), Meaning::LaunchFailed(None));
    }

    /// Each runner exit's meaning: the escapee is the stop, a set-up failure re-detects the tier
    /// with its cause, and the rest fail this launch.
    #[test]
    fn every_runner_exit_has_its_meaning() {
        for each in RunExit::ALL {
            let meaning = RunOutcome::Runner(each).meaning();
            let want = match (each, each.no_sandbox()) {
                (RunExit::Escapee, _) => Meaning::LeftSafeWorkspace,
                (_, Some(cause)) => Meaning::Redetect(cause),
                (_, None) => Meaning::LaunchFailed(Some(each)),
            };
            assert_eq!(meaning, want, "{each:?}");
        }
        assert_eq!(
            RunOutcome::Runner(RunExit::LogonRight).meaning(),
            Meaning::Redetect(NoSandbox::SetUpBlocked(crate::Blocked::LogonRight))
        );
        assert!(matches!(
            RunOutcome::Runner(RunExit::Password).meaning(),
            Meaning::Redetect(NoSandbox::SetUpFailed(_))
        ));
        assert_eq!(
            RunOutcome::Runner(RunExit::Spawn).meaning(),
            Meaning::LaunchFailed(Some(RunExit::Spawn))
        );
    }
}
