//! A lawyer's leave on the recording fakes: one leave per agent (a double `StopUsingLogin`, a
//! split arriving during one), the 30 s wait running out, a late `SignedOut` with no leave
//! waiting, a stale state queued before the leave waited, Clauseo's own login signed out, and the
//! hold lifted when the leave is dropped mid-way or `restart_agents` fails once.

use std::future::{Future as _, poll_fn};
use std::task::Poll;
use std::time::{Duration, UNIX_EPOCH};

use clauseo_agents::{Found, SigninState, Source};
use clauseo_host::AgentsStopped;
use clauseo_protocol::{CommandOutcome, ErrorCode};
use futures::future::join;

use super::SETTINGS;
use super::codex;
use super::fakes::{HostCall, Rig, rig};
use super::leave::{LEAVE, install, reused_codex, signed_in, stop_using_command};
use super::timing::until;
use crate::bridge::lock;
use crate::setup::leave::{Wait, auto_leave};
use crate::test_support::{STEP_WAIT, block_on};

const OUT: SigninState = SigninState::SignedOut { found: None };

/// The leave left the login: no longer reused anywhere, the entry rewritten as Clauseo's own.
fn ends_own(rig: &Rig) {
    assert!(!rig.backend.wiring.reused.get(&codex()));
    let calls = rig.host.calls();
    assert!(calls.contains(&HostCall::SetReusedLogin(codex(), false)));
    let entries = lock(&rig.readers.entries).clone();
    assert_eq!(entries.len(), 1, "one entry written: {entries:?}");
}

#[test]
fn a_second_stop_using_login_while_one_runs_answers_conflict_and_one_leave_runs() {
    let rig = reused_codex();
    let setup = rig.setup();
    let first = setup.run(SETTINGS, stop_using_command(&codex()));
    let second = async {
        let answer = setup.run(SETTINGS, stop_using_command(&codex())).await;
        rig.s18_sends(&codex(), OUT);
        answer
    };
    let (first, second) = rig.pumped(join(first, second));
    assert_eq!(first, Ok(CommandOutcome::Done));
    assert_eq!(second.unwrap_err().code(), ErrorCode::CommandConflict);
    assert_eq!(rig.journal(), LEAVE, "one leave, in its order");
    ends_own(&rig);
}

#[test]
fn a_split_arriving_during_stop_using_login_gets_conflict_and_the_entry_ends_own() {
    let rig = reused_codex();
    let setup = rig.setup();
    let first = setup.run(SETTINGS, stop_using_command(&codex()));
    let split = async {
        let answer = auto_leave(&rig.backend, &codex(), Some(Found::CodexLinkSplit)).await;
        rig.s18_sends(&codex(), OUT);
        answer
    };
    let (first, split) = rig.pumped(join(first, split));
    assert_eq!(first, Ok(CommandOutcome::Done));
    assert_eq!(split.unwrap_err().code(), ErrorCode::CommandConflict);
    assert_eq!(rig.journal(), LEAVE);
    ends_own(&rig);
    // The lawyer's leave left the login, so the split's leave has nothing left to do.
    let mut queue = rig.backend.wiring.leaves.queue().unwrap();
    assert!(queue.try_recv().is_err(), "no automatic leave queued again");
}

#[test]
fn a_stale_state_queued_before_the_leave_waited_never_ends_it() {
    let rig = reused_codex();
    lock(&rig.agents.answers).insert("stop_using_login", OUT);
    let setup = rig.setup();
    let answer = rig.pumped(async {
        // A status check confirmed the reused login again just before the leave waited; the
        // pump has not taken it yet. It would end the leave as "did not stop".
        let again = SigninState::SignedIn {
            email: Some("lawyer@firm.example".to_owned()),
            plan: None,
            source: Source::Reused,
            at: UNIX_EPOCH + Duration::from_secs(1),
        };
        rig.s18_sends(&codex(), again);
        setup.run(SETTINGS, stop_using_command(&codex())).await
    });
    assert_eq!(
        answer,
        Ok(CommandOutcome::Done),
        "the leave took S18's SignedOut"
    );
    assert_eq!(rig.journal(), LEAVE);
}

#[test]
fn the_lawyer_s_leave_with_no_signed_out_in_30_s_lifts_the_hold_and_answers_failed() {
    let rig = reused_codex();
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let send = setup.run(SETTINGS, stop_using_command(&codex()));
    let (answer, ()) = rig.pumped(join(send, script.fire(Wait::SignedOut)));
    let error = answer.unwrap_err();
    assert_eq!(error.code(), ErrorCode::SigninFailed);
    assert!(error.code().is_retryable());
    assert_eq!(script.elapsed(), Duration::from_secs(30));
    assert_eq!(
        rig.journal(),
        [
            "readers.hold",
            "host.stop_agents",
            "agents.stop_using_login",
            "host.restart_agents",
            "readers.release",
        ]
    );
    // Nothing else of the host is asked: no turn is dropped, and the queue goes on as
    // `restart_agents` resumes it.
    assert_eq!(
        rig.host.calls(),
        [
            HostCall::StopAgents(codex()),
            HostCall::RestartAgents(codex())
        ]
    );
    assert!(rig.backend.wiring.reused.get(&codex()), "nothing changed");
}

#[test]
fn a_late_signed_out_with_no_leave_waiting_runs_the_steps_after_the_stop() {
    let rig = reused_codex();
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let send = setup.run(SETTINGS, stop_using_command(&codex()));
    let answer = rig.pumped(async {
        let (answer, ()) = join(send, script.fire(Wait::SignedOut)).await;
        rig.journal();
        rig.s18_sends(&codex(), OUT);
        let told = || rig.host.calls().contains(&HostCall::LoginChanged(codex()));
        until("the steps after the stop", told).await;
        answer
    });
    assert_eq!(answer.unwrap_err().code(), ErrorCode::SigninFailed);
    assert_eq!(
        rig.journal(),
        [
            "host.set_reused_login",
            "host.remove_login_links",
            "host.set_agent_launch",
            "readers.set_entry",
            "host.login_changed",
        ]
    );
    ends_own(&rig);
    let restarts = rig.host.calls().into_iter();
    let restarts = restarts.filter(|call| *call == HostCall::RestartAgents(codex()));
    assert_eq!(restarts.count(), 1, "only the leave's own, when it gave up");
}

#[test]
fn clauseo_s_own_login_signed_out_restarts_the_agents_and_has_the_limits_read() {
    let rig = rig(true);
    install(&rig, &codex());
    rig.s18_was(&codex(), signed_in(Source::Own));
    rig.pumped(async {
        rig.s18_sends(&codex(), OUT);
        let told = || rig.host.calls().contains(&HostCall::LoginChanged(codex()));
        until("the sign-out's calls", told).await;
    });
    assert_eq!(
        rig.host.calls(),
        [
            HostCall::RestartAgents(codex()),
            HostCall::LoginChanged(codex())
        ]
    );
}

#[test]
fn a_lawyer_s_leave_dropped_mid_way_lifts_the_hold() {
    let rig = reused_codex();
    let setup = rig.setup();
    let mut send = Box::pin(setup.run(SETTINGS, stop_using_command(&codex())));
    block_on(poll_fn(|cx| {
        assert!(
            send.as_mut().poll(cx).is_pending(),
            "waits for S18's SignedOut"
        );
        Poll::Ready(())
    }));
    assert_eq!(rig.host.calls(), [HostCall::StopAgents(codex())]);
    let (told, restarted) = std::sync::mpsc::channel();
    *lock(&rig.host.restarted) = Some(told);
    drop(send);
    restarted
        .recv_timeout(STEP_WAIT)
        .expect("the hold lifted from its own thread");
    assert_eq!(
        rig.host.calls(),
        [
            HostCall::StopAgents(codex()),
            HostCall::RestartAgents(codex())
        ]
    );
    // The agent is free for the next leave: no waiter or slot was left behind.
    lock(&rig.agents.answers).insert("stop_using_login", OUT);
    let again = setup.run(SETTINGS, stop_using_command(&codex()));
    assert_eq!(rig.pumped(again), Ok(CommandOutcome::Done));
}

#[test]
fn a_failed_restart_is_tried_once_more_then_logged() {
    for fails in [1, 2] {
        let rig = reused_codex();
        *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
        *lock(&rig.host.restart_fails) = fails;
        let setup = rig.setup();
        let answer = rig.pumped(setup.run(SETTINGS, stop_using_command(&codex())));
        assert_eq!(
            answer.unwrap_err().code(),
            ErrorCode::SigninAgentsStillRunning
        );
        assert_eq!(
            rig.host.calls(),
            [
                HostCall::StopAgents(codex()),
                HostCall::RestartAgents(codex()),
                HostCall::RestartAgents(codex()),
            ],
            "{fails} failed: tried twice, never more"
        );
    }
}
