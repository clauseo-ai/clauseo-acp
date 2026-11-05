//! The bars on a sign-in (the orchestrator's ruling on 0048) and the join: a firm policy's
//! endpoint, a stored key or an endpoint the lawyer set refuse every start, in that order of
//! precedence, and end a running sign-in without an error; with none of them a second start
//! joins, even with nothing pumped between the two.

use std::time::UNIX_EPOCH;

use clauseo_agents::{AgentsEvent, SigninCode, SigninState};
use clauseo_protocol::{CommandOutcome, ErrorCode, SetupCommand, SignIn, SignInMethod};
use serde_json::json;

use super::fakes::{Rig, command, rig, setup_events};
use super::{KEY, MAIN, SETTINGS, claude};
use crate::bridge::lock;
use crate::setup::admit::{SignInBar, sign_in_bar};
use crate::test_support::block_on;

const ENDPOINT: &str = "https://gateway.firm.example";

fn start() -> SetupCommand {
    command("start_sign_in", &claude(), &json!({}))
}

fn set_key() -> SetupCommand {
    command("set_agent_key", &claude(), &json!({ "key": KEY }))
}

fn set_endpoint() -> SetupCommand {
    command("set_firm_endpoint", &claude(), &json!({ "url": ENDPOINT }))
}

fn waiting() -> SigninState {
    SigninState::Waiting {
        since: UNIX_EPOCH,
        nudge: false,
    }
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

/// How many times the fake `Agents` was asked `name`.
fn asked(rig: &Rig, name: &str) -> usize {
    rig.calls().iter().filter(|(call, _)| *call == name).count()
}

/// Each window's `StartSignIn` is refused with `code`, its text holding no key or address,
/// and no run starts.
fn every_start_refused(rig: &Rig, code: ErrorCode) {
    for label in [MAIN, SETTINGS] {
        let refused = block_on(rig.setup().run(label, start())).unwrap_err();
        assert_eq!(refused.code(), code, "{label}");
        assert!(!refused.retryable());
        for secret in [KEY, ENDPOINT, "gateway"] {
            assert!(!refused.message().contains(secret), "{refused}");
        }
    }
    assert_eq!(asked(rig, "sign_in"), 0, "no S18 run");
}

#[test]
fn a_firm_policy_s_endpoint_refuses_every_start_and_ends_a_running_sign_in() {
    let rig = rig(true);
    s18(&rig, waiting());
    rig.backend.set_firm_policy(&claude(), true);
    assert_eq!(
        asked(&rig, "cancel_sign_in"),
        1,
        "the running sign-in ended"
    );
    every_start_refused(&rig, ErrorCode::SigninFirmManaged);
}

#[test]
fn a_key_or_an_endpoint_refuses_every_start_with_its_own_code() {
    for (setting, code) in [
        (set_key(), ErrorCode::SigninKeyInUse),
        (set_endpoint(), ErrorCode::SigninEndpointInUse),
    ] {
        let rig = rig(true);
        let done = block_on(rig.setup().run(SETTINGS, setting));
        assert_eq!(done, Ok(CommandOutcome::Done));
        every_start_refused(&rig, code);
        assert_eq!(asked(&rig, "cancel_sign_in"), 0, "nothing ran to end");
    }
}

#[test]
fn a_key_or_an_endpoint_ends_a_running_sign_in_with_no_failure_shown() {
    for (setting, method) in [
        (set_key(), SignInMethod::ApiKey),
        (set_endpoint(), SignInMethod::Firm),
    ] {
        let rig = rig(true);
        s18(&rig, waiting());
        let mut feed = rig.events.follow();
        let done = block_on(rig.setup().run(SETTINGS, setting));
        assert_eq!(done, Ok(CommandOutcome::Done));
        assert_eq!(
            asked(&rig, "cancel_sign_in"),
            1,
            "{method:?}: through CancelSignIn's path"
        );
        let cancelled = SigninState::Failed {
            code: SigninCode::Cancelled,
            at: UNIX_EPOCH,
        };
        s18(&rig, cancelled);
        let shown: Vec<SignIn> = setup_events(&mut feed)
            .into_iter()
            .filter_map(|event| match event {
                clauseo_protocol::SetupEvent::SignInChanged { state, .. } => Some(state),
                _ => None,
            })
            .collect();
        assert!(
            !shown
                .iter()
                .any(|state| matches!(state, SignIn::Failed { .. })),
            "{method:?}: no failure reaches the window: {shown:?}"
        );
        assert!(
            matches!(shown.last(), Some(SignIn::SignedIn { method: m, .. }) if *m == method),
            "{method:?}: the new line carries the state: {shown:?}"
        );
    }
}

#[test]
fn with_no_key_or_endpoint_a_second_start_joins_the_running_one() {
    let rig = rig(true);
    let done = Ok(CommandOutcome::Done);
    assert_eq!(block_on(rig.setup().run(MAIN, start())), done);
    s18(&rig, waiting());
    assert_eq!(block_on(rig.setup().run(SETTINGS, start())), done);
    assert_eq!(asked(&rig, "sign_in"), 1, "joined, never restarted");
}

#[test]
fn two_quick_starts_with_nothing_pumped_reach_sign_in_once() {
    let rig = rig(true);
    let done = Ok(CommandOutcome::Done);
    assert_eq!(block_on(rig.setup().run(MAIN, start())), done);
    assert_eq!(block_on(rig.setup().run(SETTINGS, start())), done);
    assert_eq!(
        asked(&rig, "sign_in"),
        1,
        "the second start joined the first"
    );
    let failed = SigninState::Failed {
        code: SigninCode::Failed,
        at: UNIX_EPOCH,
    };
    s18(&rig, failed);
    assert_eq!(block_on(rig.setup().run(MAIN, start())), done);
    assert_eq!(
        asked(&rig, "sign_in"),
        2,
        "a failure ends the mark: a new start runs"
    );
}

#[test]
fn the_firm_s_policy_wins_then_a_key_then_an_endpoint() {
    use SignInBar::{EndpointInUse, FirmManaged, KeyInUse};
    for (firm, key, endpoint, bar) in [
        (false, false, false, None),
        (false, false, true, Some(EndpointInUse)),
        (false, true, false, Some(KeyInUse)),
        (false, true, true, Some(KeyInUse)),
        (true, false, false, Some(FirmManaged)),
        (true, false, true, Some(FirmManaged)),
        (true, true, false, Some(FirmManaged)),
        (true, true, true, Some(FirmManaged)),
    ] {
        assert_eq!(
            sign_in_bar(firm, key, endpoint),
            bar,
            "{firm} {key} {endpoint}"
        );
    }
    let rig = rig(true);
    for setting in [set_endpoint(), set_key()] {
        let done = block_on(rig.setup().run(SETTINGS, setting));
        assert_eq!(done, Ok(CommandOutcome::Done));
    }
    every_start_refused(&rig, ErrorCode::SigninKeyInUse);
    rig.backend.set_firm_policy(&claude(), true);
    every_start_refused(&rig, ErrorCode::SigninFirmManaged);
}
