//! R17's second review round: a write to the login during a pending sign-in refuses launches
//! until its record lands, which holds for the stamps from before (M1), and its bound passing
//! first gives unknown and a check; a sign-out through Clauseo is watched with no timer and
//! leaves launches to the agent's own sign-in refusal (M3); followers that panic past their
//! restarts pin every shared login at `agent.identity_unknown` (L4); and a sign-in through
//! Clauseo after a launch records its account, by id or by email, while no other path does.

use std::time::UNIX_EPOCH;

use clauseo_agents::{AccountHash, AccountRead, SigninState, Source, sign_in_limit};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode};
use serde_json::json;

use super::fakes::command;
use super::identity::{IdRig, refused, signed_in};
use super::identity_watch::files::remove;
use super::{SETTINGS, claude, codex};
use crate::setup::identity::{Verdict, check, expect, gate, on_state, stamps};
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

/// Claude's account id, as its status names an account with no email.
const ACCOUNT_ID: &str = "4f1c9a2e-0d6b-4c2f-9a51-7e3d2b8c6a10";

/// A `SignedIn` of Clauseo's own login with no email.
fn signed_in_nameless() -> SigninState {
    SigninState::SignedIn {
        email: None,
        plan: None,
        source: Source::Own,
        at: UNIX_EPOCH,
    }
}

/// Claude signed in through Clauseo with `ONE`, and launched past the gate.
fn launched_claude(test: &str) -> IdRig {
    let id = IdRig::new(test);
    id.signs_in(&claude(), ONE);
    assert_eq!(gate(&id.rig.backend, &claude()), None, "launched");
    id
}

#[test]
fn a_sign_in_with_no_email_after_a_launch_records_the_account_its_status_names_by_id() {
    let id = launched_claude("identity-nameless-after-launch");
    let (rig, agent) = (&id.rig, claude());
    let backend = &rig.backend;
    expect(backend, &agent);
    block_on(on_state(backend, &agent, &signed_in_nameless()));
    assert_eq!(verdict(&id, &agent), Verdict::Due);
    let by_id = AccountHash::of(ACCOUNT_ID).unwrap();
    rig.agents
        .accounts
        .answers(&agent, AccountRead::Account(by_id.clone()));
    assert_eq!(block_on(check(backend, &agent)), Verdict::Same);
    assert_eq!(backend.wiring.identity.recorded(&agent), Some(by_id));
    assert_eq!(wanted(backend, &agent), None, "never unknown");
}

#[test]
fn a_sign_in_with_an_email_after_a_launch_records_it() {
    let id = launched_claude("identity-email-after-launch");
    let (rig, agent) = (&id.rig, claude());
    let backend = &rig.backend;
    expect(backend, &agent);
    block_on(on_state(backend, &agent, &signed_in(OTHER, Source::Own)));
    assert_eq!(verdict(&id, &agent), Verdict::Same);
    let other = AccountHash::of(OTHER);
    assert_eq!(backend.wiring.identity.recorded(&agent), other);
    assert_eq!(wanted(backend, &agent), None);
}

#[test]
fn no_other_path_records_an_account_after_a_launch() {
    // Nothing recorded, launched: a `SignedIn` of S18's own status check is unknown.
    let id = IdRig::new("identity-background-after-launch");
    let (rig, agent) = (&id.rig, claude());
    let backend = &rig.backend;
    assert_eq!(gate(backend, &agent), None, "launched");
    block_on(on_state(backend, &agent, &signed_in(ONE, Source::Own)));
    assert_eq!(verdict(&id, &agent), Verdict::Unknown);
    assert_eq!(backend.wiring.identity.recorded(&agent), None);
    assert_eq!(
        wanted(backend, &agent),
        Some(ErrorCode::AgentIdentityUnknown)
    );
}

#[test]
fn a_sign_in_with_no_email_whose_record_never_lands_is_unknown_and_checked_at_its_bound() {
    let id = launched_claude("identity-nameless-lapsed");
    let (rig, agent) = (&id.rig, claude());
    let backend = &rig.backend;
    expect(backend, &agent);
    block_on(on_state(backend, &agent, &signed_in_nameless()));
    rig.agents.accounts.answers(&agent, AccountRead::Failed);
    assert_eq!(block_on(check(backend, &agent)), Verdict::Unknown);
    let reads = id.reads();
    let by_id = AccountHash::of(ACCOUNT_ID).unwrap();
    rig.agents
        .accounts
        .answers(&agent, AccountRead::Account(by_id));
    let bound = sign_in_limit(AgentKind::Claude).as_secs();
    id.look(bound + 1);
    assert_eq!(id.reads(), reads + 1, "checked at the bound");
    assert_eq!(
        verdict(&id, &agent),
        Verdict::Unknown,
        "past it, nothing records"
    );
    assert_eq!(backend.wiring.identity.recorded(&agent), None);
}
