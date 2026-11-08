//! What follows a leave that finished (`leave/after.rs`), on the recording fakes: the agents
//! restart once when a stored key, an endpoint or Clauseo's own login remains, after an
//! automatic leave's first try or a try again and after a lawyer's leave; with nothing usable
//! after a lawyer's leave; with nothing usable they stay held and the agent shows signed out. A key
//! set on a reused login restarts once, after the key is stored. An endpoint that then arrives
//! through the settings restarts a held agent once (the final security review's L3), unless a
//! leave of that agent runs: the leave's end restarts it.

use std::collections::BTreeSet;

use clauseo_agents::keys::Keychain;
use clauseo_agents::{Found, SigninState};
use clauseo_host::AgentsStopped;
use clauseo_protocol::{AgentId, CommandOutcome, ErrorCode, SignIn, SignInMethod};
use futures::future::join;
use serde_json::json;

use super::fakes::{HostCall, Rig, command, rig_keys, rig_on};
use super::leave::{reuse, stop_using_command};
use super::switching::{count, retry, shown};
use super::timing::until;
use super::{SETTINGS, claude, codex};
use crate::bridge::lock;
use crate::setup::keys::KeyCache;
use crate::setup::leave::{Wait, auto_leave};
use crate::setup::refresh::{SettingsRead, apply_settings};
use crate::test_support::block_on;

const OLD_KEY: &str = "sk-ant-api03-STORED-KEY-0001";
const NEW_KEY: &str = "sk-ant-api03-NEW-KEY-0002";

const CHANGED: SigninState = SigninState::SignedOut {
    found: Some(Found::ReusedChanged),
};

const OUT: SigninState = SigninState::SignedOut { found: None };

/// A rig with Claude reused, its key stored first when `key`.
fn reused_claude(key: bool) -> Rig {
    let keychain = Keychain::memory();
    if key {
        keychain.store(&claude(), OLD_KEY).unwrap();
    }
    let rig = rig_on(true, keychain);
    // As `start.rs` tells it from the keys loaded.
    rig.backend.compose.set_key(&claude(), key);
    reuse(&rig, &claude());
    rig
}

/// An automatic leave of Claude whose first try finishes.
fn leave_at_first_try(rig: &Rig) {
    lock(&rig.agents.answers).insert("stop_using_login", CHANGED);
    let agent = claude();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::ReusedChanged));
    assert_eq!(rig.pumped(leave), Ok(()));
}

/// The line a key or an endpoint shows, whatever its time.
fn method(shown: Option<&SignIn>) -> Option<SignInMethod> {
    match shown {
        Some(SignIn::SignedIn { method, .. }) => Some(*method),
        _ => None,
    }
}

fn restarts(rig: &Rig, agent: &AgentId) -> usize {
    count(rig, &HostCall::RestartAgents(agent.clone()))
}

/// The settings read that brings Claude's endpoint in, as the lawyer's or a firm's does.
fn endpoint_arrives() -> SettingsRead {
    SettingsRead {
        firm: BTreeSet::from([claude()]),
        ..SettingsRead::default()
    }
}

#[test]
fn a_first_try_automatic_leave_with_a_stored_key_restarts_the_agents_once() {
    let rig = reused_claude(true);
    leave_at_first_try(&rig);
    assert_eq!(restarts(&rig, &claude()), 1);
    let journal = rig.journal();
    assert_eq!(
        journal[journal.len() - 3..],
        [
            "readers.release",
            "host.restart_agents",
            "host.login_changed"
        ],
        "after the steps after the stop: {journal:?}"
    );
    assert_eq!(
        method(shown(&rig, &claude()).as_ref()),
        Some(SignInMethod::ApiKey)
    );
}

#[test]
fn a_first_try_automatic_leave_with_an_endpoint_restarts_the_agents_once() {
    let rig = reused_claude(false);
    rig.backend.compose.set_firm(&claude(), true);
    leave_at_first_try(&rig);
    assert_eq!(restarts(&rig, &claude()), 1);
    assert_eq!(
        method(shown(&rig, &claude()).as_ref()),
        Some(SignInMethod::Firm)
    );
}

#[test]
fn an_automatic_leave_with_nothing_usable_stays_held_and_shows_signed_out() {
    let rig = reused_claude(false);
    leave_at_first_try(&rig);
    let calls = rig.host.calls();
    assert!(calls.contains(&HostCall::StopAgents(claude())));
    assert_eq!(restarts(&rig, &claude()), 0, "held for the next login");
    let found = Some(clauseo_protocol::FoundLogin::ReusedChanged);
    assert_eq!(
        shown(&rig, &claude()),
        Some(SignIn::SignedOut { found }),
        "never a silent hold"
    );
}

/// An automatic leave of Claude whose first try times out and whose try again finishes.
fn leave_at_a_try_again(rig: &Rig) {
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    lock(&rig.agents.answers).insert("stop_using_login", CHANGED);
    let mut script = rig.timer.script();
    let agent = claude();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::ReusedChanged));
    let drive = async {
        script.asked(retry(5)).await;
        assert_eq!(restarts(rig, &claude()), 0, "held while it tries");
        *lock(&rig.host.stopped) = AgentsStopped::Exited;
        script.fire(retry(5)).await;
    };
    let (left, ()) = rig.pumped(join(leave, drive));
    assert_eq!(left, Ok(()));
    assert_eq!(count(rig, &HostCall::StopAgents(claude())), 2);
}

#[test]
fn a_try_again_that_finishes_with_nothing_usable_does_not_restart() {
    let rig = reused_claude(false);
    leave_at_a_try_again(&rig);
    assert_eq!(restarts(&rig, &claude()), 0, "held for the next login");
    let found = Some(clauseo_protocol::FoundLogin::ReusedChanged);
    assert_eq!(shown(&rig, &claude()), Some(SignIn::SignedOut { found }));
}

#[test]
fn a_try_again_that_finishes_with_a_stored_key_restarts_once() {
    let rig = reused_claude(true);
    leave_at_a_try_again(&rig);
    assert_eq!(restarts(&rig, &claude()), 1);
    assert_eq!(
        method(shown(&rig, &claude()).as_ref()),
        Some(SignInMethod::ApiKey)
    );
}

#[test]
fn stop_using_login_with_a_stored_key_restarts_once_and_with_none_stays_held() {
    for key in [true, false] {
        let rig = reused_claude(key);
        lock(&rig.agents.answers)
            .insert("stop_using_login", SigninState::SignedOut { found: None });
        let setup = rig.setup();
        let answer = rig.pumped(setup.run(SETTINGS, stop_using_command(&claude())));
        assert_eq!(answer, Ok(CommandOutcome::Done));
        assert_eq!(restarts(&rig, &claude()), usize::from(key), "key {key}");
        if !key {
            assert_eq!(
                shown(&rig, &claude()),
                Some(SignIn::SignedOut { found: None })
            );
        }
    }
}

#[test]
fn a_key_set_on_a_reused_login_with_a_key_stored_restarts_once_after_the_new_key() {
    let rig = reused_claude(true);
    lock(&rig.agents.answers).insert("stop_using_login", SigninState::SignedOut { found: None });
    let setup = rig.setup();
    let key = command("set_agent_key", &claude(), &json!({ "key": NEW_KEY }));
    assert_eq!(
        rig.pumped(setup.run(SETTINGS, key)),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(restarts(&rig, &claude()), 1, "the key's, not the leave's");
    let calls = rig.host.calls();
    let restart = calls
        .iter()
        .position(|call| *call == HostCall::RestartAgents(claude()));
    let left = calls
        .iter()
        .position(|call| *call == HostCall::LoginChanged(claude()));
    assert!(restart > left, "after the leave ended: {calls:?}");
    let stored = rig.keychain.load(&claude()).unwrap();
    assert_eq!(stored.as_ref().map(|key| key.as_str()), Some(NEW_KEY));
}

#[test]
fn an_endpoint_arriving_through_the_settings_restarts_a_held_agent_once() {
    let rig = reused_claude(false);
    leave_at_first_try(&rig);
    assert_eq!(restarts(&rig, &claude()), 0, "held for the next login");
    let read = endpoint_arrives();
    block_on(apply_settings(&rig.backend, read.clone()));
    assert_eq!(restarts(&rig, &claude()), 1, "the endpoint is usable");
    assert_eq!(
        method(shown(&rig, &claude()).as_ref()),
        Some(SignInMethod::Firm)
    );
    block_on(apply_settings(&rig.backend, read));
    assert_eq!(restarts(&rig, &claude()), 1, "no change, no restart");
}

#[test]
fn an_endpoint_through_the_settings_during_an_automatic_leave_waits_for_its_end() {
    let rig = reused_claude(false);
    *lock(&rig.host.stopped) = AgentsStopped::TimedOut;
    lock(&rig.agents.answers).insert("stop_using_login", CHANGED);
    let mut script = rig.timer.script();
    let agent = claude();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::ReusedChanged));
    let drive = async {
        script.asked(retry(5)).await;
        apply_settings(&rig.backend, endpoint_arrives()).await;
        assert_eq!(restarts(&rig, &claude()), 0, "held while it tries again");
        *lock(&rig.host.stopped) = AgentsStopped::Exited;
        script.fire(retry(5)).await;
    };
    let (left, ()) = rig.pumped(join(leave, drive));
    assert_eq!(left, Ok(()));
    assert_eq!(
        restarts(&rig, &claude()),
        1,
        "once, when the leave finished"
    );
    assert_eq!(
        method(shown(&rig, &claude()).as_ref()),
        Some(SignInMethod::Firm)
    );
}

#[test]
fn an_endpoint_through_the_settings_during_a_given_up_leave_waits_for_the_late_end() {
    let rig = reused_claude(false);
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let send = setup.run(SETTINGS, stop_using_command(&claude()));
    let answer = rig.pumped(async {
        let (answer, ()) = join(send, script.fire(Wait::SignedOut)).await;
        apply_settings(&rig.backend, endpoint_arrives()).await;
        assert_eq!(
            restarts(&rig, &claude()),
            0,
            "held while S18's stop job runs"
        );
        rig.s18_sends(&claude(), OUT);
        let lifted = || restarts(&rig, &claude()) == 1;
        until("the hold lifted", lifted).await;
        answer
    });
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(restarts(&rig, &claude()), 1, "once, at the late end");
    assert_eq!(
        method(shown(&rig, &claude()).as_ref()),
        Some(SignInMethod::Firm)
    );
}

#[test]
fn an_endpoint_landing_after_an_automatic_leave_read_its_facts_restarts_once() {
    let rig = reused_claude(false);
    lock(&rig.agents.answers).insert("stop_using_login", CHANGED);
    let (reached, go) = rig.backend.wiring.leaves.arm_pause("auto.free");
    let agent = claude();
    let leave = auto_leave(&rig.backend, &agent, Some(Found::ReusedChanged));
    let drive = async {
        // The leave has read its facts (nothing usable, so it keeps the hold) and waits to free.
        reached.await.expect("the leave reaches its free");
        assert_eq!(restarts(&rig, &claude()), 0, "held before the free");
        apply_settings(&rig.backend, endpoint_arrives()).await;
        assert_eq!(
            restarts(&rig, &claude()),
            0,
            "the leave restarts at its free"
        );
        let _sent = go.send(());
    };
    let (left, ()) = rig.pumped(join(leave, drive));
    assert_eq!(left, Ok(()));
    assert_eq!(restarts(&rig, &claude()), 1, "once, at the leave's free");
    assert_eq!(
        method(shown(&rig, &claude()).as_ref()),
        Some(SignInMethod::Firm)
    );
}

#[test]
fn an_endpoint_landing_after_a_given_up_leave_read_its_facts_restarts_once() {
    let rig = reused_claude(false);
    let mut script = rig.timer.script();
    let setup = rig.setup();
    let send = setup.run(SETTINGS, stop_using_command(&claude()));
    let (reached, go) = rig.backend.wiring.leaves.arm_pause("given_up.end");
    let answer = rig.pumped(async {
        let (answer, ()) = join(send, script.fire(Wait::SignedOut)).await;
        rig.s18_sends(&claude(), OUT);
        reached.await.expect("the late end reaches its end");
        assert_eq!(
            restarts(&rig, &claude()),
            0,
            "held before the late end frees"
        );
        apply_settings(&rig.backend, endpoint_arrives()).await;
        assert_eq!(restarts(&rig, &claude()), 0, "the late end restarts");
        let _sent = go.send(());
        answer
    });
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert_eq!(restarts(&rig, &claude()), 1, "once, at the late end");
    assert_eq!(
        method(shown(&rig, &claude()).as_ref()),
        Some(SignInMethod::Firm)
    );
}

#[test]
fn a_hold_lifted_while_the_key_is_unreadable_is_refused_before_the_restart() {
    let keychain = Keychain::memory();
    keychain.store(&claude(), OLD_KEY).unwrap();
    keychain.fail_reads(true);
    let keys = KeyCache::load_all(keychain.clone(), [&claude(), &codex()]);
    let rig = rig_keys(keychain, keys);
    rig.backend.compose.set_key(&claude(), true);
    reuse(&rig, &claude());
    leave_at_first_try(&rig);
    let calls = rig.host.calls();
    let at = |call: &HostCall| calls.iter().position(|seen| seen == call);
    let refused = HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyUnreadable));
    let refused = at(&refused).expect("the refusal is set");
    let restart = at(&HostCall::RestartAgents(claude())).expect("restarted");
    assert!(refused < restart, "refused first: {calls:?}");
    assert_eq!(restarts(&rig, &claude()), 1);
}
