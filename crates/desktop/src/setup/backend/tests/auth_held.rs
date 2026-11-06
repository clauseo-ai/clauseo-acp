//! A launch or a turn refused with `agent.auth_required` leaves its queue held for the sign-in;
//! the host's `ThreadsChanged` row says so, and the agent's status is checked again, at most
//! once at a time, so a login that is gone shows signed out instead of "signed in" under the
//! error.

use std::time::UNIX_EPOCH;

use clauseo_agents::{AgentsEvent, SigninState, Source};
use clauseo_protocol::{AgentId, SetupEvent, SignIn};
use futures::StreamExt as _;
use serde_json::json;

use super::codex;
use super::fakes::{Rig, rig, setup_events};
use crate::bridge::lock;
use crate::setup::host_calls::SignInHolds;
use crate::setup::start::follow_sign_in_holds;
use crate::test_support::block_on;

fn uuid(n: u128) -> String {
    uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0000 | n).to_string()
}

/// An app event: one `ThreadsChanged` of Codex threads, each `(thread, held for the sign-in)`.
fn frame(rows: &[(u128, bool)]) -> String {
    let threads: Vec<_> = rows
        .iter()
        .map(|&(thread, held)| {
            json!({
                "thread": uuid(thread),
                "matter": uuid(1),
                "agent": "codex-acp",
                "status": "idle",
                "queue_held": held,
                "queue_held_reason": held.then_some("sign_in"),
            })
        })
        .collect();
    json!({ "threads_changed": {
        "matter": uuid(1),
        "threads": threads,
        "removed": [],
        "all": false,
    }})
    .to_string()
}

/// The host's holds as the app subscription's frames carry them.
fn feed_holds(rig: &Rig, frames: Vec<String>) {
    let mut holds = SignInHolds::default();
    let stream = futures::stream::iter(frames)
        .flat_map(move |json| futures::stream::iter(holds.entered(&json)))
        .boxed();
    *lock(&rig.host.holds) = Some(stream);
}

fn status_checks(rig: &Rig) -> usize {
    rig.calls()
        .iter()
        .filter(|(call, agent)| *call == "check_status" && *agent == codex())
        .count()
}

fn signed_in() -> SigninState {
    SigninState::SignedIn {
        email: None,
        plan: None,
        source: Source::Own,
        at: UNIX_EPOCH,
    }
}

#[test]
fn a_launch_refused_auth_required_checks_the_status_once_and_shows_signed_out() {
    let rig = rig(true);
    rig.s18_was(&codex(), signed_in());
    lock(&rig.agents.answers).insert("check_status", SigninState::SignedOut { found: None });
    let mut feed = rig.events.follow();
    // the thread's row before the launch, then held for the sign-in once it is refused
    feed_holds(&rig, vec![frame(&[(2, false)]), frame(&[(2, true)])]);
    rig.pumped(follow_sign_in_holds(&rig.backend));
    assert_eq!(status_checks(&rig), 1, "one status run");
    let shown: Vec<SignIn> = setup_events(&mut feed)
        .into_iter()
        .filter_map(|event| match event {
            SetupEvent::SignInChanged { agent, state } if agent == codex() => Some(state),
            _ => None,
        })
        .collect();
    assert!(
        matches!(shown.first(), Some(SignIn::SignedIn { .. })),
        "signed in before: {shown:?}"
    );
    assert_eq!(
        shown.last(),
        Some(&SignIn::SignedOut { found: None }),
        "the fresh state is pushed: {shown:?}"
    );
}

#[test]
fn a_second_hold_while_a_status_check_is_on_its_way_starts_none() {
    let rig = rig(true);
    rig.s18_was(&codex(), signed_in());
    // no answer yet: the first check is still on its way while two more threads are held, and
    // the first thread's row comes again for another change
    feed_holds(
        &rig,
        vec![
            frame(&[(2, true)]),
            frame(&[(3, true), (4, true)]),
            frame(&[(2, true)]),
        ],
    );
    block_on(follow_sign_in_holds(&rig.backend));
    assert_eq!(status_checks(&rig), 1, "one status run at a time");

    // its state comes; a hold after that checks again
    let event = AgentsEvent::Signin {
        agent: codex(),
        state: SigninState::SignedOut { found: None },
    };
    block_on(rig.backend.pump(futures::stream::iter([event])));
    feed_holds(&rig, vec![frame(&[(5, true)])]);
    block_on(follow_sign_in_holds(&rig.backend));
    assert_eq!(
        status_checks(&rig),
        2,
        "a new hold after the state checks again"
    );
}

#[test]
fn rows_sent_again_or_held_for_another_reason_are_no_new_hold() {
    let none: Vec<AgentId> = Vec::new();
    let mut holds = SignInHolds::default();
    assert_eq!(holds.entered(&frame(&[(2, true), (3, true)])), [codex()]);
    assert_eq!(holds.entered(&frame(&[(2, true)])), none, "sent again");
    assert_eq!(holds.entered(&frame(&[(2, false)])), none, "released");
    assert_eq!(holds.entered(&frame(&[(2, true)])), [codex()], "held again");
    let paused = frame(&[(6, true)]).replace("\"sign_in\"", "\"paused\"");
    assert_eq!(holds.entered(&paused), none, "held for the lawyer");
    assert_eq!(
        holds.entered(r#"{"settings_changed":{}}"#),
        none,
        "another event"
    );
}
