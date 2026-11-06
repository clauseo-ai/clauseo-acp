//! R17's launch gate and the edges round the watcher: a fresh launch costs one stat and no
//! status, a changed login refuses before the gate answers (the review's M3); only a key or a
//! provider in the launch env exempts, never an endpoint (H3); losing an exemption checks before
//! the relaunch, and gaining one drops a check in flight (G4, A3); a first run waits for its
//! startup check and trusts no account first seen after a launch (G5); a refusal the host cannot
//! be told is told again (L1); a panicking follower fails closed (L2); and retries queued
//! together ask for one check.

use std::collections::BTreeMap;

use clauseo_agents::keys::Keychain;
use clauseo_agents::{AccountRead, Source};
use clauseo_protocol::{AgentId, ErrorCode};
use clauseo_settings::{Plan, PlanProvider};
use futures::future::join;
use serde_json::json;

use super::fakes::{HostCall, command};
use super::identity::{IdRig, refused, signed_in, stop};
use super::identity_watch::until;
use super::{SETTINGS, claude, codex};
use crate::bridge::lock;
use crate::setup::identity::{Verdict, check, exemption_changed, gate};
use crate::setup::keys::KeyNotes;
use crate::setup::refusal::{follow_keys, wanted};
use crate::test_support::block_on;

const ONE: &str = "lawyer@firm.example";
const OTHER: &str = "someone-else@elsewhere.example";
const KEY: &str = "sk-proj-GATE-TEST-NEVER-REAL-17";

fn verdict(id: &IdRig, agent: &AgentId) -> Verdict {
    id.rig.backend.wiring.identity.verdict(agent)
}

/// `agent`'s firm plan: `endpoint` and `provider`.
fn plan(id: &IdRig, agent: &AgentId, provider: Option<PlanProvider>) {
    let plan = Plan {
        endpoint: Some("https://gateway.firm.example".to_owned()),
        provider,
        env: BTreeMap::new(),
    };
    let plans = &id.rig.backend.wiring.plans;
    plans.write().unwrap().insert(agent.clone(), plan);
}

/// How long `gate` takes per call, over `times` calls each answering `None`.
#[expect(
    clippy::disallowed_methods,
    reason = "the test times the gate's real cost on this machine"
)]
fn gate_cost(id: &IdRig, agent: &AgentId, times: u32) -> std::time::Duration {
    let started = std::time::Instant::now();
    for _ in 0..times {
        assert_eq!(gate(&id.rig.backend, agent), None);
    }
    started.elapsed() / times
}

#[test]
fn a_fresh_launch_costs_a_stat_and_a_changed_login_is_refused_before_the_gate_answers() {
    let id = IdRig::new("identity-gate");
    let (rig, agent) = (&id.rig, codex());
    let mut asks = rig.backend.wiring.identity.take_asks().unwrap();
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&agent, ONE);
    rig.forget_calls();
    let cost = gate_cost(&id, &agent, 1000);
    println!("R17 timing: the launch gate, fresh stamps: {cost:?} per launch");
    assert_eq!(id.reads(), 0, "no status run");
    assert_eq!(rig.host.calls(), [], "no refusal");
    // A swap just before the launch: the refusal is the host's before the gate answers.
    id.replace_codex_login("swapped");
    assert_eq!(
        gate(&rig.backend, &agent),
        Some(ErrorCode::AgentIdentityChecking)
    );
    assert_eq!(
        rig.host.calls(),
        [refused(&agent, Some(ErrorCode::AgentIdentityChecking))]
    );
    assert_eq!(
        asks.try_recv().ok(),
        Some(agent.clone()),
        "the check asked for"
    );
    // The check confirms the account: lifted, and the next launch goes on.
    rig.agents.accounts.names(&agent, ONE);
    assert_eq!(block_on(check(&rig.backend, &agent)), Verdict::Same);
    assert_eq!(rig.host.calls().last(), Some(&refused(&agent, None)));
    assert_eq!(gate(&rig.backend, &agent), None);
    // Another swap to another account: refused, then changed.
    id.replace_codex_login("other");
    rig.agents.accounts.names(&agent, OTHER);
    assert_eq!(
        gate(&rig.backend, &agent),
        Some(ErrorCode::AgentIdentityChecking)
    );
    assert_eq!(block_on(check(&rig.backend, &agent)), Verdict::Changed);
    assert_eq!(
        gate(&rig.backend, &agent),
        Some(ErrorCode::SigninAccountChanged)
    );
}

#[test]
fn an_endpoint_alone_is_checked_and_a_key_or_a_provider_is_not() {
    let id = IdRig::new("identity-exempt");
    let rig = &id.rig;
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    // An endpoint alone, for either agent: launches still use the shared login.
    plan(&id, &codex(), None);
    plan(&id, &claude(), None);
    id.signs_in(&codex(), ONE);
    id.look(0);
    id.replace_codex_login("swapped");
    rig.agents.accounts.names(&codex(), OTHER);
    id.look(2);
    assert_eq!(id.reads(), 1, "checked");
    assert_eq!(verdict(&id, &codex()), Verdict::Changed);
    assert_eq!(verdict(&id, &claude()), Verdict::Idle, "no key login");
    // A provider: Claude's launches read no OAuth file.
    plan(&id, &claude(), Some(PlanProvider::Vertex));
    id.look(4);
    assert_eq!(verdict(&id, &claude()), Verdict::KeyLogin);
    assert_eq!(wanted(&rig.backend, &claude()), None);
    // A stored key: `identity_ids.rs`'s key-login test.
}

#[test]
fn losing_an_exemption_puts_the_check_before_the_relaunch() {
    let earlier = IdRig::new("identity-lose-key");
    earlier.signs_in(&codex(), ONE);
    let keychain = Keychain::memory();
    keychain.store(&codex(), KEY).unwrap();
    let id = IdRig::over_keys(earlier.scratch, keychain, |identity| identity);
    let (rig, agent) = (&id.rig, codex());
    id.settle_start();
    assert_eq!(verdict(&id, &agent), Verdict::KeyLogin);
    let clear = command("clear_agent_key", &agent, &json!({}));
    block_on(rig.setup().run(SETTINGS, clear)).unwrap();
    let calls = rig.host.calls();
    let checking = refused(&agent, Some(ErrorCode::AgentIdentityChecking));
    let refusal = calls.iter().position(|call| *call == checking);
    let restart = calls
        .iter()
        .position(|call| *call == HostCall::RestartAgents(agent.clone()));
    assert!(
        refusal.is_some() && refusal < restart,
        "checking before the relaunch: {calls:?}"
    );
    assert_eq!(verdict(&id, &agent), Verdict::Due);
    // A provider that goes does the same.
    plan(&id, &claude(), Some(PlanProvider::Bedrock));
    block_on(exemption_changed(&rig.backend, &claude()));
    assert_eq!(verdict(&id, &claude()), Verdict::KeyLogin);
    rig.backend.wiring.plans.write().unwrap().remove(&claude());
    block_on(exemption_changed(&rig.backend, &claude()));
    assert_eq!(
        wanted(&rig.backend, &claude()),
        Some(ErrorCode::AgentIdentityChecking)
    );
}

#[test]
fn a_key_login_that_starts_drops_the_check_in_flight() {
    let id = IdRig::new("identity-key-overtakes");
    let (rig, agent) = (&id.rig, claude());
    id.signs_in(&agent, ONE);
    let open = rig.agents.accounts.hold(&agent);
    rig.agents.accounts.names(&agent, OTHER);
    let backend = &rig.backend;
    let provider = async {
        until("the check runs", || {
            verdict(&id, &agent) == Verdict::Checking
        })
        .await;
        plan(&id, &agent, Some(PlanProvider::Bedrock));
        exemption_changed(backend, &agent).await;
        open.send(()).unwrap();
    };
    let (ended, ()) = block_on(join(check(backend, &agent), provider));
    assert_eq!(ended, Verdict::KeyLogin);
    assert!(!rig.host.calls().contains(&stop(&agent)), "nothing stopped");
}

#[test]
fn a_first_run_waits_for_its_startup_check_which_records() {
    let id = IdRig::over(IdRig::scratch("identity-first-wait"), |identity| identity);
    let rig = &id.rig;
    for agent in [claude(), codex()] {
        assert_eq!(
            wanted(&rig.backend, &agent),
            Some(ErrorCode::AgentIdentityChecking),
            "{agent}: refused until the startup check"
        );
    }
    rig.agents.accounts.names(&codex(), ONE);
    rig.agents
        .accounts
        .answers(&claude(), AccountRead::NoAccount);
    id.look(0);
    assert_eq!(
        rig.backend.wiring.identity.recorded(&codex()),
        clauseo_agents::AccountHash::of(ONE)
    );
    assert_eq!(wanted(&rig.backend, &codex()), None);
    assert_eq!(verdict(&id, &claude()), Verdict::Idle, "nothing signed in");
    assert_eq!(wanted(&rig.backend, &claude()), None);
}

#[test]
fn an_account_first_seen_after_a_launch_is_never_recorded() {
    let id = IdRig::new("identity-after-launch");
    let (rig, agent) = (&id.rig, codex());
    // A launch passes the gate with nothing recorded (the startup check named no account).
    assert_eq!(gate(&rig.backend, &agent), None);
    rig.pumped(async {
        rig.s18_sends(&agent, signed_in(ONE, Source::Own));
    });
    assert_eq!(rig.backend.wiring.identity.recorded(&agent), None);
    assert_eq!(verdict(&id, &agent), Verdict::Unknown);
    assert_eq!(
        wanted(&rig.backend, &agent),
        Some(ErrorCode::AgentIdentityUnknown)
    );
}

#[test]
fn a_refusal_the_host_cannot_be_told_is_told_again_at_the_next_look() {
    let id = IdRig::new("identity-unsynced");
    let (rig, agent) = (&id.rig, codex());
    id.signs_in(&agent, ONE);
    *lock(&rig.host.refused_fails) = 4;
    rig.agents.accounts.names(&agent, OTHER);
    assert_eq!(block_on(check(&rig.backend, &agent)), Verdict::Changed);
    assert!(lock(&rig.backend.wiring.unsynced).contains(&agent));
    assert!(
        rig.host.calls().contains(&stop(&agent)),
        "the stop holds it"
    );
    id.look(2);
    assert!(lock(&rig.backend.wiring.unsynced).is_empty());
    assert_eq!(
        rig.host.calls().last(),
        Some(&refused(&agent, Some(ErrorCode::SigninAccountChanged)))
    );
}

#[test]
fn a_follower_that_panics_fails_closed_and_starts_again() {
    let id = IdRig::new("identity-panic");
    let rig = &id.rig;
    let mut runs = 0;
    crate::setup::supervise::supervise(&rig.backend, || {
        runs += 1;
        assert!(runs > 1, "the injected panic");
    });
    assert_eq!(runs, 2, "started again once");
    for agent in [claude(), codex()] {
        assert_eq!(verdict(&id, &agent), Verdict::Unknown);
        let unknown = refused(&agent, Some(ErrorCode::AgentIdentityUnknown));
        assert!(rig.host.calls().contains(&unknown), "{agent}");
    }
}

#[test]
fn retries_queued_together_ask_for_one_check_each() {
    let id = IdRig::new("identity-coalesce");
    let backend = &id.rig.backend;
    let mut asks = backend.wiring.identity.take_asks().unwrap();
    let notes = KeyNotes::new();
    let retry = notes.retry_hook();
    for _ in 0..5 {
        retry(&codex(), ErrorCode::AgentIdentityChecking);
    }
    retry(&claude(), ErrorCode::AgentIdentityChecking);
    retry(&claude(), ErrorCode::AgentIdentityChecking);
    let mut receiver = notes.take().unwrap();
    drop((retry, notes));
    block_on(follow_keys(backend, &mut receiver));
    let mut asked = Vec::new();
    while let Ok(agent) = asks.try_recv() {
        asked.push(agent);
    }
    assert_eq!(asked, [codex(), claude()]);
}
