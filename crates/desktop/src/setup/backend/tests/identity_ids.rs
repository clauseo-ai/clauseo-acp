//! R17's identity when the status names no email (the orchestrator's ruling, 2026-11-06): an
//! account id is enough; a stored API key is what launches use, so the check does not apply
//! (a key login, not checked); with neither an email nor an id, and no key, it is unknown.

use clauseo_agents::keys::Keychain;
use clauseo_agents::{AccountHash, AccountRead, SigninState, Source};
use clauseo_protocol::{AgentId, ErrorCode};

use super::fakes::HostCall;
use super::identity::{IdRig, signed_in};
use super::{claude, codex};
use crate::setup::identity::{Seen, Verdict, check, on_state};
use crate::setup::refusal::wanted;
use crate::test_support::block_on;

const ONE: &str = "lawyer@firm.example";
const OTHER: &str = "someone-else@elsewhere.example";
const KEY: &str = "sk-proj-IDENTITY-TEST-NEVER-REAL-41";

/// `SignedIn` with no email, as a status that names none gives it.
fn no_email() -> SigninState {
    SigninState::SignedIn {
        email: None,
        plan: None,
        source: Source::Own,
        at: std::time::UNIX_EPOCH,
    }
}

/// S18's status `state` for `agent`, through the pump.
fn status(id: &IdRig, agent: &AgentId, state: SigninState) {
    let rig = &id.rig;
    rig.pumped(async {
        rig.s18_sends(agent, state);
    });
}

fn stopped(id: &IdRig, agent: &AgentId) -> bool {
    id.rig
        .host
        .calls()
        .contains(&HostCall::StopAgents(agent.clone()))
}

#[test]
fn an_id_only_status_names_the_account_same_then_changed() {
    let id = IdRig::new("identity-id-only");
    let (backend, agent) = (&id.rig.backend, claude());
    let mut asks = backend.wiring.identity.take_asks().unwrap();
    // A sign-in through Clauseo whose `SignedIn` names no email: nothing is recorded yet, and
    // a check is asked for, whose status names the account by its id.
    backend.wiring.identity.expect(&agent);
    assert_eq!(
        block_on(on_state(backend, &agent, &no_email())),
        Seen::SignedIn
    );
    assert_eq!(backend.wiring.identity.verdict(&agent), Verdict::Due);
    assert_eq!(asks.try_recv().ok(), Some(agent.clone()));
    assert_eq!(
        wanted(backend, &agent),
        Some(ErrorCode::AgentIdentityChecking)
    );
    id.rig.agents.accounts.names(&agent, "uuid-1");
    assert_eq!(
        block_on(check(backend, &agent)),
        Verdict::Same,
        "recorded by its id"
    );
    assert_eq!(
        backend.wiring.identity.recorded(&agent),
        AccountHash::of("uuid-1")
    );
    assert_eq!(wanted(backend, &agent), None);
    // A later status with no email asks again; the same id is the same account.
    status(&id, &agent, no_email());
    assert_eq!(asks.try_recv().ok(), Some(agent.clone()));
    assert_eq!(block_on(check(backend, &agent)), Verdict::Same);
    assert!(!stopped(&id, &agent));
    // Another id is another account.
    id.rig.agents.accounts.names(&agent, "uuid-2");
    assert_eq!(block_on(check(backend, &agent)), Verdict::Changed);
    assert!(stopped(&id, &agent));
    assert_eq!(
        wanted(backend, &agent),
        Some(ErrorCode::SigninAccountChanged)
    );
}

#[test]
fn a_key_login_is_not_checked_and_is_checked_again_once_the_key_goes() {
    let earlier = IdRig::new("identity-key-login");
    earlier.signs_in(&codex(), ONE);
    let keychain = Keychain::memory();
    keychain.store(&codex(), KEY).unwrap();
    let id = IdRig::over_keys(earlier.scratch, keychain, |identity| identity);
    let (backend, agent) = (&id.rig.backend, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    // The key is what launches use: no refusal, no status run, whatever the login does.
    assert_eq!(wanted(backend, &agent), None);
    id.look(0);
    assert_eq!(backend.wiring.identity.verdict(&agent), Verdict::KeyLogin);
    id.replace_codex_login("swapped");
    id.rig.agents.accounts.names(&agent, OTHER);
    id.look(2);
    assert_eq!(block_on(check(backend, &agent)), Verdict::KeyLogin);
    status(&id, &agent, signed_in(OTHER, Source::Own));
    assert_eq!(id.reads(), 0, "not checked");
    assert!(!stopped(&id, &agent));
    let refusals = id.rig.host.calls();
    assert!(
        !refusals
            .iter()
            .any(|call| matches!(call, HostCall::SetLaunchRefused(..))),
        "{refusals:?}"
    );
    // The key is cleared: the login is what launches use again, checked before the next.
    block_on(backend.keys.clear(&agent, |_| {})).unwrap();
    assert_eq!(
        wanted(backend, &agent),
        Some(ErrorCode::AgentIdentityChecking)
    );
    id.look(4);
    assert_eq!(id.reads(), 1);
    assert_eq!(backend.wiring.identity.verdict(&agent), Verdict::Changed);
    assert!(stopped(&id, &agent));
}

#[test]
fn no_email_and_no_id_without_a_key_is_unknown() {
    let earlier = IdRig::new("identity-no-account");
    earlier.signs_in(&codex(), ONE);
    let id = IdRig::over(earlier.scratch, |identity| identity);
    let (backend, agent) = (&id.rig.backend, codex());
    status(&id, &agent, no_email());
    assert_eq!(backend.wiring.identity.verdict(&agent), Verdict::Due);
    id.rig
        .agents
        .accounts
        .answers(&agent, AccountRead::NoAccount);
    assert_eq!(block_on(check(backend, &agent)), Verdict::Unknown);
    assert_eq!(
        wanted(backend, &agent),
        Some(ErrorCode::AgentIdentityUnknown)
    );
    assert!(!stopped(&id, &agent), "refused, retryable; nothing stopped");
    // A sign-in through Clauseo with neither is unknown too, never the old account's.
    backend.wiring.identity.expect(&agent);
    block_on(on_state(backend, &agent, &no_email()));
    assert_eq!(backend.wiring.identity.recorded(&agent), None);
    assert_eq!(block_on(check(backend, &agent)), Verdict::Unknown);
}
