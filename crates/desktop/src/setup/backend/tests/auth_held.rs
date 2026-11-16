//! A launch or a turn refused with `agent.auth_required` leaves its queue held for the sign-in;
//! the host's `ThreadsChanged` row says so, and the agent's status is checked again, at most
//! once at a time, so a login that is gone shows signed out instead of "signed in" under the
//! error.

use std::time::{Duration, UNIX_EPOCH};

use clauseo_agents::{AgentsEvent, SigninState, Source};
use clauseo_protocol::{AgentId, CommandOutcome, SetupEvent, SignIn, SignInMethod, Timestamp};
use futures::StreamExt as _;
use serde_json::json;

use super::fakes::{Rig, command, rig, setup_events};
use super::{SETTINGS, codex};
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

/// Review L5: a status check that ends with no state (it could not tell) still ends the
/// re-check, so a later hold asks again instead of waiting for a state that never comes.
#[test]
fn a_status_check_that_ends_with_no_state_lets_a_later_hold_check_again() {
    let rig = rig(true);
    rig.s18_was(&codex(), signed_in());
    feed_holds(&rig, vec![frame(&[(2, true)])]);
    block_on(follow_sign_in_holds(&rig.backend));
    assert_eq!(status_checks(&rig), 1);

    // the job ends with the state unchanged
    let event = AgentsEvent::SigninUnchanged { agent: codex() };
    block_on(rig.backend.pump(futures::stream::iter([event])));
    feed_holds(&rig, vec![frame(&[(3, true)])]);
    block_on(follow_sign_in_holds(&rig.backend));
    assert_eq!(
        status_checks(&rig),
        2,
        "a new hold after a job with no state checks again"
    );
}

/// The `SignInChanged` states sent for Codex.
fn codex_changes(events: Vec<SetupEvent>) -> Vec<SignIn> {
    events
        .into_iter()
        .filter_map(|event| match event {
            SetupEvent::SignInChanged { agent, state } if agent == codex() => Some(state),
            _ => None,
        })
        .collect()
}

/// Codex signed in to `email` with Clauseo's own login, confirmed at `secs` after the epoch.
fn signed_in_as(email: &str, secs: u64) -> SigninState {
    SigninState::SignedIn {
        email: Some(email.to_owned()),
        plan: Some("plus".to_owned()),
        source: Source::Own,
        at: UNIX_EPOCH + Duration::from_secs(secs),
    }
}

/// The `at` of a `SignedIn`.
fn at(state: Option<&SignIn>) -> Timestamp {
    match state {
        Some(SignIn::SignedIn { at, .. }) => *at,
        other => panic!("not signed in: {other:?}"),
    }
}

/// The desktop review: a re-check after `auth_required` that re-confirms the same account (it
/// reads the local login file, which an expired token still passes) keeps the first `at`, so
/// the card never shows "Signed in again" with nothing done; nothing is sent.
#[test]
fn a_recheck_that_re_confirms_the_same_account_keeps_its_time() {
    let rig = rig(true);
    let mut feed = rig.events.follow();
    rig.s18_was(&codex(), signed_in_as("a@example.com", 100));
    assert!(
        !codex_changes(setup_events(&mut feed)).is_empty(),
        "shown first"
    );
    lock(&rig.agents.answers).insert("check_status", signed_in_as("a@example.com", 200));
    feed_holds(&rig, vec![frame(&[(2, true)])]);
    rig.pumped(follow_sign_in_holds(&rig.backend));
    assert_eq!(status_checks(&rig), 1, "one status run");
    assert_eq!(
        codex_changes(setup_events(&mut feed)),
        [],
        "nothing is sent"
    );
    assert_eq!(
        rig.backend.compose.s18(&codex()),
        Some(signed_in_as("a@example.com", 100)),
        "the first time is kept"
    );
}

/// The same re-check answering another account is a real change: sent, with its new `at`.
#[test]
fn a_recheck_that_finds_another_account_is_sent_with_its_new_time() {
    let rig = rig(true);
    let mut feed = rig.events.follow();
    rig.s18_was(&codex(), signed_in_as("a@example.com", 100));
    let first = at(codex_changes(setup_events(&mut feed)).last());
    lock(&rig.agents.answers).insert("check_status", signed_in_as("b@example.com", 200));
    feed_holds(&rig, vec![frame(&[(2, true)])]);
    rig.pumped(follow_sign_in_holds(&rig.backend));
    let shown = codex_changes(setup_events(&mut feed));
    assert!(
        matches!(shown.last(), Some(SignIn::SignedIn { email: Some(email), .. }) if email == "b@example.com"),
        "{shown:?}"
    );
    assert_ne!(at(shown.last()), first, "a new time");
}

/// The latest `SignInChanged` for Codex, as a new follower gets it.
fn shown(rig: &Rig) -> Option<SignIn> {
    let mut feed = rig.events.follow();
    codex_changes(setup_events(&mut feed)).pop()
}

/// The Windows test's defect 4: a turn refused `agent.auth_required` while a key is in memory
/// marks the key's line `refused` (the status check, kept, reads only the plan login), so the
/// Settings window can say the key was not accepted; saving a key or removing it clears it.
#[test]
fn a_hold_with_a_key_in_memory_marks_the_key_refused_until_a_key_is_saved_or_removed() {
    let rig = rig(true);
    let set_key = |key: &str| command("set_agent_key", &codex(), &json!({ "key": key }));
    rig.s18_was(&codex(), SigninState::SignedOut { found: None });
    assert_eq!(
        block_on(
            rig.setup()
                .run(SETTINGS, set_key("sk-proj-NEVER-IN-A-LOG-0001"))
        ),
        Ok(CommandOutcome::Done)
    );
    assert!(
        matches!(
            shown(&rig),
            Some(SignIn::SignedIn {
                method: SignInMethod::ApiKey,
                refused: None,
                ..
            })
        ),
        "{:?}",
        shown(&rig)
    );
    lock(&rig.agents.answers).insert("check_status", SigninState::SignedOut { found: None });
    feed_holds(&rig, vec![frame(&[(2, true)])]);
    rig.pumped(follow_sign_in_holds(&rig.backend));
    assert_eq!(status_checks(&rig), 1, "the status check still runs");
    assert!(
        matches!(
            shown(&rig),
            Some(SignIn::SignedIn {
                method: SignInMethod::ApiKey,
                refused: Some(_),
                ..
            })
        ),
        "the refusal is published: {:?}",
        shown(&rig)
    );

    assert_eq!(
        block_on(
            rig.setup()
                .run(SETTINGS, set_key("sk-proj-NEVER-IN-A-LOG-0002"))
        ),
        Ok(CommandOutcome::Done)
    );
    assert!(
        matches!(
            shown(&rig),
            Some(SignIn::SignedIn {
                method: SignInMethod::ApiKey,
                refused: None,
                ..
            })
        ),
        "a saved key clears it: {:?}",
        shown(&rig)
    );

    rig.backend.recheck_sign_in(&codex());
    assert!(matches!(
        shown(&rig),
        Some(SignIn::SignedIn {
            refused: Some(_),
            ..
        })
    ));
    let clear = command("clear_agent_key", &codex(), &json!({}));
    assert_eq!(
        rig.pumped(rig.setup().run(SETTINGS, clear)),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(
        shown(&rig),
        Some(SignIn::SignedOut { found: None }),
        "a removed key takes its refusal with it"
    );
}

/// With no key in memory a hold marks nothing: a plan's line never carries `refused`.
#[test]
fn a_hold_with_no_key_marks_no_refusal() {
    let rig = rig(true);
    rig.s18_was(&codex(), signed_in());
    lock(&rig.agents.answers).insert("check_status", signed_in());
    feed_holds(&rig, vec![frame(&[(2, true)])]);
    rig.pumped(follow_sign_in_holds(&rig.backend));
    assert!(
        matches!(
            shown(&rig),
            Some(SignIn::SignedIn {
                method: SignInMethod::Plan,
                refused: None,
                ..
            })
        ),
        "{:?}",
        shown(&rig)
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
