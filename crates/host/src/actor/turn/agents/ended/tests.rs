//! The detach an agent's end logs, and whether its process left the Safe workspace: the
//! Escapee exit is checked first, whoever stopped the agent (R17 PR-E review), and only for
//! an agent launched while the computer had tier B.

use clauseo_protocol::DetachReason;

use super::{StopCause, detach_of};
use crate::agent::{Ended, StopStep};

/// `clauseo-sandbox-run.exe`'s Escapee exit as Windows' `ExitStatus::code` gives it.
const ESCAPEE: i32 = 0xE0C1_500F_u32.cast_signed();

fn exit(code: i32) -> DetachReason {
    DetachReason::Exited {
        code: Some(code),
        stderr_tail: String::new(),
    }
}

const CAUSES: [StopCause; 4] = [
    StopCause::Asked,
    StopCause::Restarting,
    StopCause::IdleStopped,
    StopCause::Closed,
];

#[test]
fn an_escapee_exit_is_the_detach_whoever_stopped_the_agent() {
    for cause in CAUSES {
        for step in [
            StopStep::StdinClosed,
            StopStep::Killed,
            StopStep::Unresponsive,
        ] {
            let ended = Ended::Stopped {
                step,
                report: exit(ESCAPEE),
            };
            assert_eq!(
                detach_of(ended, cause, true),
                (exit(ESCAPEE), true),
                "{cause:?} {step:?}"
            );
        }
    }
    assert_eq!(
        detach_of(Ended::Exited(exit(ESCAPEE)), StopCause::Asked, true),
        (exit(ESCAPEE), true)
    );
}

#[test]
fn another_exit_keeps_the_host_s_stop_and_an_agent_without_tier_b_never_escapes() {
    let stopped = |report| Ended::Stopped {
        step: StopStep::StdinClosed,
        report,
    };
    let host_stops = [
        (StopCause::Asked, DetachReason::Closed),
        (StopCause::Restarting, DetachReason::Restarting),
        (StopCause::IdleStopped, DetachReason::IdleStopped),
        (StopCause::Closed, exit(1)),
    ];
    for (cause, detach) in host_stops.clone() {
        assert_eq!(detach_of(stopped(exit(1)), cause, true), (detach, false));
    }
    // Launched at tier C (or before the sandbox runner exists): the code is any program's.
    for (cause, detach) in host_stops {
        let detach = if cause == StopCause::Closed {
            exit(ESCAPEE)
        } else {
            detach
        };
        assert_eq!(
            detach_of(stopped(exit(ESCAPEE)), cause, false),
            (detach, false)
        );
    }
    assert_eq!(
        detach_of(Ended::Exited(exit(ESCAPEE)), StopCause::Asked, false),
        (exit(ESCAPEE), false)
    );
}
