//! Sign-in races after #458's review: a key or an endpoint set before S18's first state still
//! cancels the reserved job, with no failure shown, and a start that ends unreported (its job
//! cancelled, or replaced by a `FindLogin` or a `SignOut`) leaves no starting mark, so the
//! next `StartSignIn` runs.

use std::sync::Arc;
use std::time::UNIX_EPOCH;

use clauseo_agents::{AgentsEvent, SigninCode, SigninState};
use clauseo_protocol::{CommandOutcome, HostError, SetupEvent, SignIn, SignInMethod};
use futures::channel::mpsc::UnboundedReceiver;
use serde_json::json;

use super::fakes::{Rig, command, rig, setup_events};
use super::{KEY, MAIN, SETTINGS, claude};
use crate::bridge::lock;
use crate::test_support::block_on;

const DONE: Result<CommandOutcome, HostError> = Ok(CommandOutcome::Done);

/// Claude's command `name` with `fields`, from the window `label`.
fn send(
    rig: &Rig,
    label: &str,
    name: &str,
    fields: &serde_json::Value,
) -> Result<CommandOutcome, HostError> {
    block_on(rig.setup().run(label, command(name, &claude(), fields)))
}

fn start(rig: &Rig, label: &str) -> Result<CommandOutcome, HostError> {
    send(rig, label, "start_sign_in", &json!({}))
}

/// S18's `state` for Claude, as the fake answers `signin_state` and as the pump sends it.
fn s18(rig: &Rig, state: SigninState) {
    lock(&rig.agents.signin).insert(claude(), state.clone());
    let event = AgentsEvent::Signin {
        agent: claude(),
        state,
    };
    block_on(rig.backend.pump(futures::stream::iter([event])));
}

fn failed(code: SigninCode) -> SigninState {
    SigninState::Failed {
        code,
        at: UNIX_EPOCH,
    }
}

fn signed_out() -> SigninState {
    SigninState::SignedOut { found: None }
}

/// How many times the fake `Agents` was asked `name`.
fn asked(rig: &Rig, name: &str) -> usize {
    rig.calls().iter().filter(|(call, _)| *call == name).count()
}

/// The sign-in states `feed` got.
fn shown(feed: &mut UnboundedReceiver<Arc<str>>) -> Vec<SignIn> {
    setup_events(feed)
        .into_iter()
        .filter_map(|event| match event {
            SetupEvent::SignInChanged { state, .. } => Some(state),
            _ => None,
        })
        .collect()
}

#[test]
fn a_key_set_before_s18_s_first_state_cancels_the_job_quietly_and_a_later_start_runs() {
    let rig = rig(true);
    let mut feed = rig.events.follow();
    let mut texts = rig.events.follow();
    assert_eq!(start(&rig, MAIN), DONE);
    assert_eq!(
        send(&rig, SETTINGS, "set_agent_key", &json!({ "key": KEY })),
        DONE
    );
    assert_eq!(
        asked(&rig, "cancel_sign_in"),
        1,
        "the reserved job is cancelled"
    );
    s18(&rig, failed(SigninCode::Cancelled));
    let shown = shown(&mut feed);
    assert!(
        !shown
            .iter()
            .any(|state| matches!(state, SignIn::Failed { .. })),
        "no failure reaches the window: {shown:?}"
    );
    let texts: Vec<Arc<str>> = std::iter::from_fn(|| texts.try_recv().ok()).collect();
    assert!(
        !texts.iter().any(|text| text.contains("signin.cancelled")),
        "{texts:?}"
    );
    assert!(
        matches!(
            shown.last(),
            Some(SignIn::SignedIn {
                method: SignInMethod::ApiKey,
                ..
            })
        ),
        "the key's line carries the state: {shown:?}"
    );
    assert_eq!(send(&rig, SETTINGS, "clear_agent_key", &json!({})), DONE);
    assert_eq!(start(&rig, MAIN), DONE);
    assert_eq!(
        asked(&rig, "sign_in"),
        2,
        "a new sign-in, not joined to the dead one"
    );
}

#[test]
fn a_key_from_settings_after_main_s_start_with_nothing_pumped_cancels_once() {
    let rig = rig(true);
    assert_eq!(start(&rig, MAIN), DONE);
    assert_eq!(
        send(&rig, SETTINGS, "set_agent_key", &json!({ "key": KEY })),
        DONE
    );
    assert_eq!(asked(&rig, "cancel_sign_in"), 1);
    assert_eq!(send(&rig, SETTINGS, "clear_agent_key", &json!({})), DONE);
    let mut feed = rig.events.follow();
    assert_eq!(start(&rig, MAIN), DONE);
    assert_eq!(
        asked(&rig, "sign_in"),
        2,
        "the dead job reported nothing; a new one runs"
    );
    s18(&rig, failed(SigninCode::Failed));
    assert!(
        matches!(shown(&mut feed).last(), Some(SignIn::Failed { .. })),
        "the new job's failure is shown: the replaced job's end is no longer awaited"
    );
}

#[test]
fn a_start_then_signed_out_then_a_start_reaches_sign_in_twice() {
    let rig = rig(true);
    assert_eq!(start(&rig, MAIN), DONE);
    s18(&rig, signed_out());
    assert_eq!(start(&rig, MAIN), DONE);
    assert_eq!(asked(&rig, "sign_in"), 2, "a resting state ends the mark");
}

#[test]
fn a_start_replaced_by_find_login_or_sign_out_before_it_reports_leaves_no_mark() {
    for replace in ["find_login", "sign_out"] {
        let rig = rig(true);
        s18(&rig, signed_out());
        assert_eq!(start(&rig, MAIN), DONE, "{replace}");
        assert_eq!(send(&rig, SETTINGS, replace, &json!({})), DONE, "{replace}");
        // The replacing job's end: the same state as before, so nothing new is sent.
        let mut feed = rig.events.follow();
        let _replay = shown(&mut feed);
        s18(&rig, signed_out());
        assert_eq!(shown(&mut feed), [], "{replace}: unchanged, not sent");
        assert_eq!(start(&rig, MAIN), DONE, "{replace}");
        assert_eq!(asked(&rig, "sign_in"), 2, "{replace}: a new start runs");
    }
}
