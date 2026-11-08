//! R17 PR-E: a blocked sandbox, or one another Windows account owns, refuses every agent launch
//! on this computer with `sandbox.blocked`, whatever `sandbox.required` or
//! `sandbox.allow_unprotected_auto` say, and offers no administrator retry; a cause the
//! settings cover still runs with both settings as they let it.

use std::sync::Arc;

use clauseo_protocol::{
    ErrorCode, HostCommand, NotProtectedCause, Protection, StopReason, StreamId,
};
use clauseo_sandbox::{Blocked, Detection, NoSandbox, SET_UP_BY_OTHER_USER, SetupState};

use crate::codex::{Recording, turn_failed};
use crate::protection::dir;
use crate::protection_rig::{Plan, c, rig, through, view};
use crate::support::{Dir, FAKE, Scenario};

/// The settings that let an agent run without the sandbox, in an auto mode too.
fn lenient(dir: &Dir) {
    dir.set_settings(
        &[(FAKE, Scenario::Bundled("prompt-stream"))],
        serde_json::json!({"sandbox": {"required": false, "allow_unprotected_auto": true}}),
    );
}

/// Each detection that blocks the sandbox, the set-up state beside it, and its wire cause.
fn blocked_cases() -> [(Detection, SetupState, NotProtectedCause); 4] {
    [
        (
            c(NoSandbox::SetUpBlocked(Blocked::Policy)),
            SetupState::Blocked(Blocked::Policy),
            NotProtectedCause::Blocked {
                reason: "policy".to_owned(),
            },
        ),
        (
            c(NoSandbox::SetUpBlocked(Blocked::AdminCopyMissing)),
            SetupState::Blocked(Blocked::AdminCopyMissing),
            NotProtectedCause::Blocked {
                reason: "admin_copy_missing".to_owned(),
            },
        ),
        (
            c(NoSandbox::SetUpBlocked(Blocked::OtherAccount)),
            SetupState::Blocked(Blocked::OtherAccount),
            NotProtectedCause::OtherAccount,
        ),
        (
            c(NoSandbox::SetUpFailed(SET_UP_BY_OTHER_USER.to_owned())),
            SetupState::SetUpByOtherUser,
            NotProtectedCause::OtherAccount,
        ),
    ]
}

/// The view names the block with no "Try again", and `CreateThread` is refused with
/// `sandbox.blocked` though both settings would let an unprotected agent run.
#[test]
fn a_blocked_sandbox_refuses_create_thread_whatever_the_settings() {
    let dir = dir("protection_blocked_create");
    lenient(&dir);
    for (run, (detection, state, cause)) in (0u16..).zip(blocked_cases()) {
        let (host, _rig) =
            crate::protection::start(&dir, run, Plan::new(vec![detection], vec![state]));
        let answer = view(&host);
        assert_eq!(
            answer.computer,
            Protection::NotProtected { cause },
            "run {run}"
        );
        assert!(
            !answer.retry_asks_admin,
            "run {run}: a set-up can't lift a block"
        );
        assert!(!answer.required && answer.unprotected_auto_allowed);
        let matter = host.matters()[0].matter;
        let command = HostCommand::CreateThread {
            matter,
            agent: clauseo_protocol::AgentId::new(FAKE).unwrap(),
        };
        drop(host.refused(command, ErrorCode::SandboxBlocked));
        host.shutdown();
    }
}

/// A queued turn's launch meets the block too: the turn fails with `sandbox.blocked` and no
/// agent starts. A cause the settings cover (a set-up not done yet) runs the same turn.
#[test]
fn a_blocked_sandbox_fails_a_turn_and_a_needed_set_up_still_runs_it() {
    let dir = dir("protection_blocked_turn");
    let host = dir.start(0);
    let matter = host.create_matter("Smith", &dir.folder("smith", &[]));
    let thread = host.create_thread(matter, FAKE);
    host.shutdown();
    lenient(&dir);
    let needed = (
        c(NoSandbox::NeedsSetUp),
        SetupState::Needed,
        NotProtectedCause::NeedsSetUp,
    );
    let cases = blocked_cases().into_iter().chain([needed]);
    for (run, (detection, state, cause)) in (1u16..).zip(cases) {
        let recording = Arc::new(Recording::new(&dir));
        let launches = Arc::clone(&recording.envs);
        let sandbox = rig(Plan::new(vec![detection], vec![state])).sandbox;
        let sandbox = through(sandbox, Arc::clone(&recording) as _);
        let host = dir.start_launching(run, recording, move |config| config.with_sandbox(sandbox));
        let mut events = host.events(StreamId::Thread(thread));
        let (turn, _) = host.prompt(thread, "Look at the matter.");
        let ended = events.until_turn_ended(turn);
        if cause == NotProtectedCause::NeedsSetUp {
            assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
            assert_eq!(launches.lock().unwrap().len(), 1, "the agent did not start");
        } else {
            assert_eq!(ended.stop, StopReason::Failed, "run {run}");
            let failed = turn_failed(&events, turn).unwrap_or_else(|| panic!("{}", events.dump()));
            assert_eq!(failed.code, ErrorCode::SandboxBlocked, "run {run}");
            assert_eq!(failed.at, None);
            assert!(
                launches.lock().unwrap().is_empty(),
                "run {run}: an agent started"
            );
        }
        host.shutdown();
    }
}
