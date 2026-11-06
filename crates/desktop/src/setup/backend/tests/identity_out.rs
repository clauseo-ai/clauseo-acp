//! R17's second review round: a write to the login during a pending sign-in refuses launches
//! until its record lands, which holds for the stamps from before (M1), and its bound passing
//! first gives unknown and a check; a sign-out through Clauseo is watched with no timer and
//! leaves launches to the agent's own sign-in refusal (M3); followers that panic past their
//! restarts pin every shared login at `agent.identity_unknown` (L4); and a sign-in through
//! Clauseo after a launch records its account, by id or by email, while no other path does; and
//! a firm-managed agent (the e2e build's) is outside the check, the lawyer's own entry never.

use std::time::UNIX_EPOCH;

use clauseo_agents::{AccountHash, AccountRead, SigninState, Source, sign_in_limit};
use clauseo_protocol::{AgentId, AgentKind, ErrorCode};
use clauseo_settings::{Effective, Locations, SystemFiles};
use serde_json::json;

use super::fakes::command;
use super::identity::{IdRig, refused, signed_in};
use super::identity_watch::files::remove;
use super::{SETTINGS, claude, codex};
use crate::bridge::lock;
use crate::setup::identity::{Verdict, check, expect, gate, on_state, stamps};
use crate::setup::refresh::{SettingsRead, apply_settings};
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

#[test]
fn a_write_in_a_no_email_sign_in_s_window_breaks_it_and_nothing_is_recorded() {
    // The reviewer's trigger: garbage written, the first check fails, then another account.
    let id = launched_claude("identity-nameless-broken");
    let (rig, agent) = (&id.rig, claude());
    let backend = &rig.backend;
    expect(backend, &agent);
    block_on(on_state(backend, &agent, &signed_in_nameless()));
    id.scratch.file(&claude_store(), b"garbage");
    rig.agents.accounts.answers(&agent, AccountRead::Failed);
    id.look(2);
    assert_eq!(verdict(&id, &agent), Verdict::Unknown);
    id.scratch
        .file(&claude_store(), b"{ \"the attacker's\": \"login\" }");
    rig.agents.accounts.names(&agent, OTHER);
    id.look(4);
    id.look(40);
    assert_eq!(
        backend.wiring.identity.recorded(&agent),
        None,
        "nothing recorded"
    );
    assert_eq!(verdict(&id, &agent), Verdict::Unknown);
    assert_eq!(
        gate(backend, &agent),
        Some(ErrorCode::AgentIdentityUnknown),
        "a launch is refused"
    );
}

#[test]
fn a_transient_failure_with_the_files_unchanged_keeps_the_window_and_a_retry_records() {
    let id = launched_claude("identity-nameless-retry");
    let (rig, agent) = (&id.rig, claude());
    let backend = &rig.backend;
    expect(backend, &agent);
    block_on(on_state(backend, &agent, &signed_in_nameless()));
    rig.agents.accounts.answers(&agent, AccountRead::Failed);
    assert_eq!(block_on(check(backend, &agent)), Verdict::Unknown);
    let by_id = AccountHash::of(ACCOUNT_ID).unwrap();
    rig.agents
        .accounts
        .answers(&agent, AccountRead::Account(by_id.clone()));
    // The 30 s retry after a failed status.
    id.look(31);
    assert_eq!(verdict(&id, &agent), Verdict::Same);
    assert_eq!(backend.wiring.identity.recorded(&agent), Some(by_id));
    assert_eq!(wanted(backend, &agent), None);
}

/// The e2e build's smoke: its agents are firm-managed (`E2E_AGENTS_MANAGED`, as a policy lock on
/// `agents.<id>` makes them), with no Clauseo install, so no status read can run. A managed
/// agent is not checked and never refused by the identity check, at its first launch or at a
/// relaunch after its agent exited mid-turn.
#[test]
fn a_managed_agent_is_never_checked_nor_refused_launch_after_launch() {
    let id = IdRig::over(IdRig::scratch("identity-managed"), |identity| identity);
    let (rig, backend) = (&id.rig, &id.rig.backend);
    let agents = [claude(), codex()];
    lock(&backend.wiring.applied).managed.extend(agents.clone());
    for agent in &agents {
        rig.agents.accounts.answers(agent, AccountRead::Failed);
    }
    for secs in [0, 2, 60] {
        id.look(secs);
        for agent in &agents {
            assert_eq!(wanted(backend, agent), None, "{agent} at {secs}s");
            // A launch, the agent exiting mid-turn, and the relaunch asking again.
            for launch in 0..3 {
                assert_eq!(gate(backend, agent), None, "{agent}, launch {launch}");
            }
        }
    }
    assert_eq!(id.reads(), 0, "no status read");
    let identity = |call: &super::fakes::HostCall| {
        matches!(
            call,
            super::fakes::HostCall::SetLaunchRefused(
                _,
                Some(
                    ErrorCode::AgentIdentityChecking
                        | ErrorCode::AgentIdentityUnknown
                        | ErrorCode::SigninAccountChanged
                )
            )
        )
    };
    let calls = rig.host.calls();
    assert!(!calls.iter().any(identity), "{calls:?}");
}

/// The lawyer's own `agents.<id>`, however it is spelled (Clauseo's path through `.`, or a
/// command of its own), a session could write: never firm-managed, so an agent's verdict stays
/// what it was, Same or Unknown, and it stays checked.
#[test]
fn the_lawyer_s_own_entry_however_spelled_stays_checked() {
    let id = IdRig::new("identity-own-entry");
    let (rig, backend) = (&id.rig, &id.rig.backend);
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&codex(), ONE);
    id.signs_in(&claude(), ONE);
    rig.agents.accounts.answers(&claude(), AccountRead::Failed);
    assert_eq!(block_on(check(backend, &claude())), Verdict::Unknown);
    let data = id.scratch.path().join("data");
    let odd = data
        .join(".")
        .join("agents")
        .join("codex-acp")
        .join("codex-acp");
    let own = id.scratch.path().join("bin").join("claude-acp");
    let text = json!({ "agents": {
        "codex-acp": { "command": odd },
        "claude-acp": { "command": own },
    }});
    id.scratch
        .file("data/settings.json", text.to_string().as_bytes());
    let locations = Locations::under_root(&data);
    let effective = Effective::load(&locations, &SystemFiles, &[]);
    assert!(effective.agent("codex-acp").is_some() && effective.agent("claude-acp").is_some());
    let read = SettingsRead::read(&effective, backend.known.keys());
    assert!(
        read.managed.is_empty(),
        "the lawyer's entries are not managed"
    );
    block_on(apply_settings(backend, read));
    assert_eq!(verdict(&id, &codex()), Verdict::Same);
    assert_eq!(verdict(&id, &claude()), Verdict::Unknown);
    assert_eq!(
        gate(backend, &claude()),
        Some(ErrorCode::AgentIdentityUnknown),
        "still refused"
    );
    id.replace_codex_login("swapped");
    assert_eq!(
        gate(backend, &codex()),
        Some(ErrorCode::AgentIdentityChecking),
        "still checked"
    );
}

#[test]
fn on_the_keychain_path_a_no_email_sign_in_s_window_closes_at_its_first_check() {
    // macOS Claude may keep its login in the Keychain, which no stamp sees.
    let id = IdRig::with("identity-nameless-keychain", |identity| {
        identity.with_keychain_fallback(true)
    });
    let (rig, agent) = (&id.rig, claude());
    let backend = &rig.backend;
    id.signs_in(&agent, ONE);
    assert_eq!(gate(backend, &agent), None, "launched");
    expect(backend, &agent);
    block_on(on_state(backend, &agent, &signed_in_nameless()));
    rig.agents.accounts.answers(&agent, AccountRead::Failed);
    assert_eq!(block_on(check(backend, &agent)), Verdict::Unknown);
    let by_id = AccountHash::of(ACCOUNT_ID).unwrap();
    rig.agents
        .accounts
        .answers(&agent, AccountRead::Account(by_id));
    id.look(31);
    assert_eq!(id.reads(), 2, "the retry ran");
    assert_eq!(
        verdict(&id, &agent),
        Verdict::Unknown,
        "and recorded nothing"
    );
    assert_eq!(backend.wiring.identity.recorded(&agent), None);
}
