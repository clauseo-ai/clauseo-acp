//! R17's second review round: a write to the login during a pending sign-in refuses launches
//! until its record lands, which holds for the stamps from before (M1), and its bound passing
//! first gives unknown and a check; a sign-out through Clauseo is watched with no timer and
//! leaves launches to the agent's own sign-in refusal (M3); followers that panic past their
//! restarts pin every shared login at `agent.identity_unknown` (L4).

use clauseo_agents::{AccountRead, SigninState, Source, sign_in_limit};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode};
use serde_json::json;

use super::fakes::command;
use super::identity::{IdRig, refused, signed_in};
use super::identity_watch::files::remove;
use super::{SETTINGS, claude, codex};
use crate::setup::identity::{Verdict, expect, gate, on_state, stamps};
use crate::setup::refusal::wanted;
use crate::setup::supervise::{RESTARTS, supervise};
use crate::test_support::block_on;

const ONE: &str = "lawyer@firm.example";
const OTHER: &str = "someone-else@elsewhere.example";

fn verdict(id: &IdRig, agent: &AgentId) -> Verdict {
    id.rig.backend.wiring.identity.verdict(agent)
}

/// Claude's store file in Clauseo's home, relative to the scratch folder.
fn claude_store() -> String {
    let [store, ..] = clauseo_sandbox::CLAUDE_STORE_FILES;
    format!("homes/claude-acp/{store}")
}

#[test]
fn a_write_during_a_pending_sign_in_refuses_launches_until_its_record_lands() {
    let id = IdRig::new("identity-pending-write");
    let (rig, agent) = (&id.rig, claude());
    let backend = &rig.backend;
    id.scratch.file(&claude_store(), b"{}");
    id.signs_in(&agent, ONE);
    rig.agents.accounts.names(&agent, ONE);
    id.look(0);
    assert_eq!(gate(backend, &agent), None);
    // The lawyer starts a sign-in and leaves; a session writes the store meanwhile.
    expect(backend, &agent);
    let before = stamps(backend, &agent);
    id.scratch
        .file(&claude_store(), b"{ \"written\": \"by a session\" }");
    let reads = id.reads();
    // A launch in another matter is refused, before any status run.
    assert_eq!(
        gate(backend, &agent),
        Some(ErrorCode::AgentIdentityChecking)
    );
    assert_eq!(
        rig.host.calls().last(),
        Some(&refused(&agent, Some(ErrorCode::AgentIdentityChecking)))
    );
    id.look(2);
    id.look(60);
    assert_eq!(verdict(&id, &agent), Verdict::Due, "held while pending");
    assert_eq!(id.reads(), reads, "a sign-in under way is not checked");
    // The record lands: the refusal lifts, and the baseline is the stamps from before.
    block_on(on_state(backend, &agent, &signed_in(ONE, Source::Own)));
    assert_eq!(verdict(&id, &agent), Verdict::Same);
    assert_eq!(wanted(backend, &agent), None);
    assert_eq!(rig.host.calls().last(), Some(&refused(&agent, None)));
    let held = backend.wiring.identity.held_stamps(&agent);
    assert_eq!(held, Some(before));
}

#[test]
fn a_pending_sign_in_whose_record_never_lands_is_unknown_and_checked_at_its_bound() {
    let id = IdRig::new("identity-pending-lapsed");
    let (rig, agent) = (&id.rig, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&agent, ONE);
    rig.agents.accounts.names(&agent, ONE);
    id.look(0);
    expect(&rig.backend, &agent);
    id.replace_codex_login("written");
    id.look(2);
    assert_eq!(verdict(&id, &agent), Verdict::Due);
    let reads = id.reads();
    let bound = sign_in_limit(AgentKind::Codex).as_secs();
    id.look(bound + 1);
    let unknown = refused(&agent, Some(ErrorCode::AgentIdentityUnknown));
    assert!(rig.host.calls().contains(&unknown), "unknown at the bound");
    assert_eq!(id.reads(), reads + 1, "and checked");
    assert_eq!(verdict(&id, &agent), Verdict::Same);
}

#[test]
fn a_sign_out_through_clauseo_is_watched_with_no_timer_and_never_refuses_unknown() {
    let id = IdRig::new("identity-sign-out");
    let (rig, agent) = (&id.rig, codex());
    let backend = &rig.backend;
    let login = id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&agent, ONE);
    rig.agents.accounts.names(&agent, ONE);
    id.look(0);
    let sign_out = command("sign_out", &agent, &json!({}));
    assert!(block_on(rig.setup().run(SETTINGS, sign_out)).is_ok());
    remove(&login);
    let out = SigninState::SignedOut { found: None };
    block_on(on_state(backend, &agent, &out));
    assert_eq!(verdict(&id, &agent), Verdict::SignedOut);
    rig.forget_calls();
    let reads = id.reads();
    // The file's going is seen and checked once: no account, signed out.
    rig.agents.accounts.answers(&agent, AccountRead::NoAccount);
    id.look(2);
    assert_eq!(id.reads(), reads + 1);
    assert_eq!(verdict(&id, &agent), Verdict::SignedOut);
    // No status run for five minutes.
    for secs in (4..=300).step_by(2) {
        id.look(secs);
    }
    assert_eq!(id.reads(), reads + 1, "no timer after a sign-out");
    // A launch meets no identity refusal: the agent's own sign-in one
    // (`agent.auth_required`, the host's sign-in hold) is what it gets.
    assert_eq!(gate(backend, &agent), None);
    assert_eq!(wanted(backend, &agent), None);
    let unknown = refused(&agent, Some(ErrorCode::AgentIdentityUnknown));
    assert!(!rig.host.calls().contains(&unknown));
    // Another account's file comes back: checked at once, and a change.
    id.scratch.file("homes/codex-acp/auth.json", b"{ }");
    rig.agents.accounts.names(&agent, OTHER);
    id.look(302);
    assert_eq!(id.reads(), reads + 2);
    assert_eq!(verdict(&id, &agent), Verdict::Changed);
}

#[test]
fn followers_that_panic_past_their_restarts_pin_every_shared_login_at_unknown() {
    let id = IdRig::new("identity-dead");
    let (rig, backend) = (&id.rig, &id.rig.backend);
    let mut runs = 0;
    supervise(backend, || {
        runs += 1;
        panic!("the injected panic {runs}");
    });
    assert_eq!(runs, RESTARTS + 1);
    for agent in [claude(), codex()] {
        let code = Some(ErrorCode::AgentIdentityUnknown);
        assert_eq!(wanted(backend, &agent), code, "{agent}");
        assert_eq!(gate(backend, &agent), code, "{agent}");
    }
    // The pump cannot lift it: a sign-in's record lands, and the refusal stays.
    let agent = codex();
    id.signs_in(&agent, ONE);
    assert_eq!(verdict(&id, &agent), Verdict::Same);
    assert_eq!(
        wanted(backend, &agent),
        Some(ErrorCode::AgentIdentityUnknown)
    );
    assert!(!rig.host.calls().contains(&refused(&agent, None)));
}
