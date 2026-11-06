//! A lawyer's leave on the recording fakes: one leave per agent (a double `StopUsingLogin`, a
//! split arriving during one), the 30 s wait running out (`StopUsingLogin` answers done, the
//! agent shows `Switching`, then `stuck`, and the holds stay until S18's stop job ends: a late
//! `SignedOut` runs the steps after the stop, then the rule of `after.rs` (held here, with nothing
//! usable); a job that ends still reused or `Failed` only lifts it; a key whose leave gives up
//! answers `command.conflict`), a stale state queued before the leave waited, Clauseo's own
//! login signed out, and the hold lifted when the leave is dropped mid-way or `restart_agents`
//! fails once.

use std::future::{Future as _, poll_fn};
use std::task::Poll;
use std::time::{Duration, UNIX_EPOCH};

use clauseo_agents::{Found, SigninCode, SigninState, Source};
use clauseo_host::AgentsStopped;
use clauseo_protocol::{CommandOutcome, ErrorCode, SetupEvent, SignIn};
use futures::StreamExt as _;
use futures::future::join;

use super::SETTINGS;
use super::fakes::{HostCall, Rig, command, rig};
use super::leave::{LEAVE, install, reused_codex, signed_in, stop_using_command};
use super::timing::until;
use super::{claude, codex};
use crate::bridge::lock;
use crate::setup::leave::{Wait, auto_leave};
use crate::test_support::{STEP_WAIT, block_on, within};

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
fn the_lawyer_s_leave_with_no_signed_out_in_30_s_answers_done_shows_switching_and_keeps_the_holds()
{
    let rig = reused_codex();
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let send = setup.run(SETTINGS, stop_using_command(&codex()));
    let (answer, ()) = rig.pumped(join(send, script.fire(Wait::SignedOut)));
    assert_eq!(
        answer,
        Ok(CommandOutcome::Done),
        "accepted; S18's stop job runs on"
    );
    assert_eq!(script.elapsed(), Duration::from_secs(30));
    assert_eq!(shown(&rig), Some(switching_since(30, false)));
    // S18's stop job may still end the reuse, and S18 cannot cancel it: no agent relaunches
    // until it ends, so nothing changes under running agents.
    assert_eq!(
        rig.journal(),
        [
            "readers.hold",
            "host.stop_agents",
            "agents.stop_using_login"
        ]
    );
    assert_eq!(rig.host.calls(), [HostCall::StopAgents(codex())]);
    assert!(
        rig.backend.wiring.reused.get(&codex()),
        "nothing changed yet"
    );
    let again = rig.pumped(setup.run(SETTINGS, stop_using_command(&codex())));
    assert_eq!(
        again.unwrap_err().code(),
        ErrorCode::CommandConflict,
        "the slot stays taken until the job ends"
    );
    assert_eq!(rig.host.calls(), [HostCall::StopAgents(codex())]);
    let mut feed = rig.events.follow();
    block_on(script.fire(Wait::Stuck));
    let stuck = Some(switching_since(30, true));
    within("stuck at 2 minutes", async {
        while let Some(text) = feed.next().await {
            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
            let event = serde_json::from_value(value["setup"].clone());
            if let Ok(SetupEvent::SignInChanged { agent, state }) = event
                && agent == codex()
                && Some(state) == stuck
            {
                return;
            }
        }
        panic!("the events ended before stuck");
    });
    assert_eq!(
        script.elapsed(),
        Duration::from_secs(150),
        "2 minutes after the give-up"
    );
}

#[test]
fn a_give_up_whose_job_ends_still_reused_only_lifts_the_hold() {
    let rig = reused_codex();
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let send = setup.run(SETTINGS, stop_using_command(&codex()));
    let answer = rig.pumped(async {
        let (answer, ()) = join(send, script.fire(Wait::SignedOut)).await;
        rig.journal();
        rig.s18_sends(&codex(), signed_in(Source::Reused));
        let lifted = || rig.host.calls().contains(&HostCall::RestartAgents(codex()));
        until("the hold lifted", lifted).await;
        answer
    });
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(rig.journal(), ["host.restart_agents", "readers.release"]);
    assert!(
        !matches!(shown(&rig), Some(SignIn::Switching { .. })),
        "Switching ended"
    );
    assert!(rig.backend.wiring.reused.get(&codex()), "nothing changed");
    assert!(
        rig.backend.wiring.leaves.begin(&codex()).is_ok(),
        "the slot is free"
    );
}

#[test]
fn a_late_signed_out_after_a_give_up_runs_the_steps_after_the_stop_then_follows_the_rule() {
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
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(
        shown(&rig),
        Some(SignIn::SignedOut { found: None }),
        "Switching ended"
    );
    assert_eq!(
        rig.journal(),
        [
            "host.set_reused_login",
            "host.remove_login_links",
            "host.set_agent_launch",
            "readers.set_entry",
            "readers.release",
            "host.login_changed",
        ],
        "the leave finished with nothing usable: held for the next login"
    );
    ends_own(&rig);
    let restarts = rig.host.calls().into_iter();
    let restarts = restarts.filter(|call| *call == HostCall::RestartAgents(codex()));
    assert_eq!(restarts.count(), 0, "no restart");
    assert!(
        rig.backend.wiring.leaves.begin(&codex()).is_ok(),
        "the slot is free"
    );
}

#[test]
fn a_late_failed_after_a_give_up_only_lifts_the_hold_and_ends_switching() {
    let rig = reused_codex();
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let send = setup.run(SETTINGS, stop_using_command(&codex()));
    rig.pumped(async {
        let (answer, ()) = join(send, script.fire(Wait::SignedOut)).await;
        assert_eq!(answer, Ok(CommandOutcome::Done));
        rig.journal();
        let failed = SigninState::Failed {
            code: SigninCode::Failed,
            at: UNIX_EPOCH,
        };
        rig.s18_sends(&codex(), failed);
        let lifted = || rig.host.calls().contains(&HostCall::RestartAgents(codex()));
        until("the hold lifted", lifted).await;
        for _ in 0..50 {
            super::timing::yield_now().await;
        }
    });
    assert_eq!(
        rig.journal(),
        ["host.restart_agents", "readers.release"],
        "the stop failed: no steps after it, nothing else"
    );
    assert!(rig.backend.wiring.reused.get(&codex()), "still reused");
    assert!(lock(&rig.readers.entries).is_empty(), "no entry rewritten");
    assert!(
        !matches!(shown(&rig), Some(SignIn::Switching { .. })),
        "Switching ended"
    );
}

#[test]
fn a_key_whose_leave_gives_up_answers_conflict_and_stores_nothing() {
    let rig = rig(true);
    super::leave::reuse(&rig, &claude());
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let key = command(
        "set_agent_key",
        &claude(),
        &serde_json::json!({ "key": "sk-ant-api03-SETTINGS-KEY-0002" }),
    );
    let send = setup.run(SETTINGS, key);
    let (answer, ()) = rig.pumped(join(send, script.fire(Wait::SignedOut)));
    assert_eq!(answer.unwrap_err().code(), ErrorCode::CommandConflict);
    assert!(
        rig.keychain.load(&claude()).unwrap().is_none(),
        "no key stored"
    );
    assert!(rig.backend.keys.get(&claude()).is_none());
    assert_eq!(
        rig.host.calls(),
        [HostCall::StopAgents(claude())],
        "held, nothing more"
    );
    let feed = &mut rig.events.follow();
    let shows = super::fakes::setup_events(feed)
        .into_iter()
        .rev()
        .find_map(|event| match event {
            SetupEvent::SignInChanged { agent, state } if agent == claude() => Some(state),
            _ => None,
        });
    assert!(matches!(shows, Some(SignIn::Switching { .. })), "{shows:?}");
}

/// Codex's latest `SignInChanged`, as a new follower gets it.
fn shown(rig: &Rig) -> Option<SignIn> {
    let feed = &mut rig.events.follow();
    super::fakes::setup_events(feed)
        .into_iter()
        .rev()
        .find_map(|event| match event {
            SetupEvent::SignInChanged { agent, state } if agent == codex() => Some(state),
            _ => None,
        })
}

/// `Switching` since `seconds` after the rig's start.
fn switching_since(seconds: u64, stuck: bool) -> SignIn {
    let since = super::timing::start() + Duration::from_secs(seconds);
    SignIn::Switching {
        since: crate::setup::compose::timestamp(since),
        stuck,
    }
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
