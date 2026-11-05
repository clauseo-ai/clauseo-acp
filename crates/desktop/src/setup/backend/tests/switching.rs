//! Automatic leaves on the recording fakes, the waits driven by hand (`timing.rs`): S18 ending a
//! reuse by itself (`ReusedChanged`), a split whose stop times out (the hold kept, `Switching`
//! shown across two tries again, then the try that finishes restarting the agents), `stuck` at
//! 2 minutes, another agent unaffected, a new sign-in ending the tries, and a lawyer's leave told
//! apart from an automatic one.

use std::time::Duration;

use clauseo_agents::{Found, SigninState, Source};
use clauseo_host::AgentsStopped;
use clauseo_protocol::{AgentId, ErrorCode, SetupEvent, SignIn, SignInSource, Timestamp};
use futures::future::{Either, join, select};

use super::fakes::{HostCall, Rig, home, rig, setup_events};
use super::leave::{reuse, reused_codex, stop_using_command};
use super::timing::{Script, start, until};
use super::{SETTINGS, claude, codex};
use crate::bridge::lock;
use crate::setup::backend::launch;
use crate::setup::compose::timestamp;
use crate::setup::leave::{Wait, auto_leave};

const SPLIT: SigninState = SigninState::SignedOut {
    found: Some(Found::CodexLinkSplit),
};

fn retry(seconds: u64) -> Wait {
    Wait::Retry(Duration::from_secs(seconds))
}

/// The latest `SignInChanged` for `agent`, as a new follower gets it.
fn shown(rig: &Rig, agent: &AgentId) -> Option<SignIn> {
    let mut feed = rig.events.follow();
    sign_ins(&mut feed, agent).pop()
}

/// The `SignInChanged` states for `agent` that `feed` got.
fn sign_ins(
    feed: &mut futures::channel::mpsc::UnboundedReceiver<std::sync::Arc<str>>,
    agent: &AgentId,
) -> Vec<SignIn> {
    setup_events(feed)
        .into_iter()
        .filter_map(|event| match event {
            SetupEvent::SignInChanged { agent: of, state } if of == *agent => Some(state),
            _ => None,
        })
        .collect()
}

fn switching(stuck: bool) -> SignIn {
    SignIn::Switching {
        since: timestamp(start()),
        stuck,
    }
}

fn count(rig: &Rig, call: &HostCall) -> usize {
    rig.host.calls().iter().filter(|seen| *seen == call).count()
}

/// Runs `drive` beside an automatic leave of Codex for a split that never finishes.
fn beside_a_split<T>(rig: &Rig, drive: impl Future<Output = T>) -> T {
    let agent = codex();
    let leave = std::pin::pin!(auto_leave(
        &rig.backend,
        &agent,
        Some(Found::CodexLinkSplit)
    ));
    rig.pumped(async {
        match select(leave, std::pin::pin!(drive)).await {
            Either::Left((left, _)) => panic!("the leave ended: {left:?}"),
            Either::Right((out, _)) => out,
        }
    })
}

#[test]
fn reused_changed_rewrites_the_entry_at_once_then_an_automatic_leave_stops_the_agents() {
    let rig = rig(true);
    reuse(&rig, &claude());
    let changed = SigninState::SignedOut {
        found: Some(Found::ReusedChanged),
    };
    lock(&rig.agents.answers).insert("stop_using_login", changed.clone());
    rig.pumped_with_leaves(async {
        rig.s18_sends(&claude(), changed);
        let done = || count(&rig, &HostCall::LoginChanged(claude())) == 2;
        until("the automatic leave", done).await;
    });
    assert_eq!(
        rig.journal(),
        [
            // In the pump, at once: no launch reads the keychain's new account from here on.
            "host.set_reused_login",
            "host.set_agent_launch",
            "readers.set_entry",
            "host.login_changed",
            // The automatic leave, on its own task: the running agents stop and stay held.
            "readers.hold",
            "host.stop_agents",
            "agents.stop_using_login",
            "host.set_reused_login",
            "host.set_agent_launch",
            "readers.set_entry",
            "readers.release",
            "host.login_changed",
        ]
    );
    let installed = lock(&rig.agents.installs)[&claude()].clone();
    let own = launch(&claude(), &installed, &home(&claude()), SignInSource::Own).unwrap();
    for call in rig.host.calls() {
        if let HostCall::SetAgentLaunch(_, entry) = call {
            assert_eq!(entry, own, "only Clauseo's own login's entry is written");
        }
    }
    assert_eq!(count(&rig, &HostCall::RestartAgents(claude())), 0, "held");
    assert!(!rig.backend.wiring.reused.get(&claude()));
    let found = Some(clauseo_protocol::FoundLogin::ReusedChanged);
    assert_eq!(shown(&rig, &claude()), Some(SignIn::SignedOut { found }));
}

#[test]
fn a_stop_that_times_out_lifts_the_hold_for_a_lawyer_s_leave_and_keeps_it_for_a_split() {
    let lawyers = reused_codex();
    *lock(&lawyers.host.stopped) = AgentsStopped::TimedOut;
    let setup = lawyers.setup();
    let answer = lawyers.pumped(setup.run(SETTINGS, stop_using_command(&codex())));
    assert_eq!(
        answer.unwrap_err().code(),
        ErrorCode::SigninAgentsStillRunning
    );
    assert_eq!(
        lawyers.host.calls(),
        [
            HostCall::StopAgents(codex()),
            HostCall::RestartAgents(codex())
        ]
    );
    assert_eq!(shown(&lawyers, &codex()), Some(reused_line()));

    let split = reused_codex();
    *lock(&split.host.stopped) = AgentsStopped::TimedOut;
    let mut script = split.timer.script();
    beside_a_split(&split, script.asked(retry(5)));
    assert_eq!(
        split.host.calls(),
        [HostCall::StopAgents(codex())],
        "no restart"
    );
    assert_eq!(shown(&split, &codex()), Some(switching(false)));
}

/// Codex's reused line, as the rig starts.
fn reused_line() -> SignIn {
    SignIn::SignedIn {
        email: Some("lawyer@firm.example".to_owned()),
        plan: None,
        method: clauseo_protocol::SignInMethod::Plan,
        source: SignInSource::Reused,
        at: Timestamp::ZERO,
    }
}

#[test]
fn a_split_shows_switching_and_stays_held_across_two_tries_again_then_restarts() {
    let rig = reused_codex();
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    lock(&rig.agents.answers).insert("stop_using_login", SPLIT);
    let mut feed = rig.events.follow();
    drop(sign_ins(&mut feed, &codex()));
    let mut script = rig.timer.script();
    let agent = codex();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::CodexLinkSplit));
    let held = |script: &Script, tries: usize| {
        assert_eq!(count(&rig, &HostCall::StopAgents(codex())), tries);
        assert_eq!(count(&rig, &HostCall::RestartAgents(codex())), 0, "held");
        assert_eq!(
            shown(&rig, &codex()),
            Some(switching(false)),
            "at {:?}",
            script.elapsed()
        );
    };
    let drive = async {
        script.asked(retry(5)).await;
        held(&script, 1);
        script.fire(retry(5)).await;
        script.asked(retry(30)).await;
        held(&script, 2);
        script.fire(retry(30)).await;
        script.asked(retry(300)).await;
        held(&script, 3);
        *lock(&rig.host.stopped) = AgentsStopped::Exited;
        script.fire(retry(300)).await;
    };
    let (left, ()) = rig.pumped(join(leave, drive));
    assert_eq!(left, Ok(()));
    assert_eq!(script.elapsed(), Duration::from_secs(335));
    let found = Some(clauseo_protocol::FoundLogin::CodexLinkSplit);
    assert_eq!(
        sign_ins(&mut feed, &codex()),
        [switching(false), SignIn::SignedOut { found }],
        "Switching the whole time, from the first try's end; then the state after the leave"
    );
    let calls = rig.host.calls();
    assert_eq!(
        calls[calls.len() - 2..],
        [
            HostCall::RestartAgents(codex()),
            HostCall::LoginChanged(codex())
        ],
        "the held questions run once the try again finished"
    );
    assert!(!rig.backend.wiring.reused.get(&codex()));
}

#[test]
fn stuck_turns_true_at_two_minutes_and_stays_across_a_try_again() {
    let rig = reused_codex();
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    let mut script = rig.timer.script();
    beside_a_split(&rig, async {
        script.asked(retry(5)).await;
        assert_eq!(shown(&rig, &codex()), Some(switching(false)));
        script.fire(retry(5)).await;
        script.fire(retry(30)).await;
        script.asked(retry(300)).await;
        assert_eq!(script.elapsed(), Duration::from_secs(35));
        assert_eq!(
            shown(&rig, &codex()),
            Some(switching(false)),
            "before 2 minutes"
        );
        script.fire(Wait::Stuck).await;
        assert_eq!(script.elapsed(), Duration::from_secs(120));
        assert_eq!(shown(&rig, &codex()), Some(switching(true)), "at 2 minutes");
        script.fire(retry(300)).await;
        script.asked(retry(300)).await;
        assert_eq!(count(&rig, &HostCall::StopAgents(codex())), 4);
        assert_eq!(
            shown(&rig, &codex()),
            Some(switching(true)),
            "after a try again"
        );
    });
}

#[test]
fn another_agent_leaves_and_shows_as_usual_while_one_is_switching() {
    let rig = reused_codex();
    reuse(&rig, &claude());
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let answer = beside_a_split(&rig, async {
        script.asked(retry(5)).await;
        *lock(&rig.host.stopped) = AgentsStopped::Exited;
        lock(&rig.agents.answers)
            .insert("stop_using_login", SigninState::SignedOut { found: None });
        setup.run(SETTINGS, stop_using_command(&claude())).await
    });
    assert_eq!(answer, Ok(clauseo_protocol::CommandOutcome::Done));
    assert_eq!(shown(&rig, &codex()), Some(switching(false)));
    assert_eq!(
        shown(&rig, &claude()),
        Some(SignIn::SignedOut { found: None })
    );
    let codex_calls: Vec<_> = rig
        .host
        .calls()
        .into_iter()
        .filter(|call| format!("{call:?}").contains("codex-acp"))
        .collect();
    assert_eq!(
        codex_calls,
        [HostCall::StopAgents(codex())],
        "Codex alone is held"
    );
    assert!(rig.host.calls().contains(&HostCall::StopAgents(claude())));
    assert!(!rig.backend.wiring.reused.get(&claude()));
}

#[test]
fn a_new_sign_in_ends_the_tries_and_switching() {
    let rig = reused_codex();
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    let mut script = rig.timer.script();
    let agent = codex();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::CodexLinkSplit));
    let own = SigninState::SignedIn {
        email: Some("clauseo@firm.example".to_owned()),
        plan: None,
        source: Source::Own,
        at: std::time::UNIX_EPOCH,
    };
    let drive = async {
        script.asked(retry(5)).await;
        rig.s18_sends(&codex(), own);
    };
    let (left, ()) = rig.pumped(join(leave, drive));
    assert_eq!(left, Ok(()));
    assert!(matches!(
        shown(&rig, &codex()),
        Some(SignIn::SignedIn {
            source: SignInSource::Own,
            ..
        })
    ));
    assert_eq!(
        count(&rig, &HostCall::StopAgents(codex())),
        1,
        "no try again"
    );
    assert_eq!(
        count(&rig, &HostCall::RestartAgents(codex())),
        1,
        "the sign-in's"
    );
}
