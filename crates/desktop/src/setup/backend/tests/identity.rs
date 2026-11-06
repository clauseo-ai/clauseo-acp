//! R17's identity check through the backend (`setup/identity.rs`): a swap of the shared
//! login's account while sessions run stops the agents and says sign in again; a login file
//! that changes with the same account checks and lifts; a status that fails is unknown; a
//! fresh verdict costs a launch nothing; a Keychain-only login checks again before a launch
//! once stale; and the host's launch retry is dispatched by its code.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};

use clauseo_agents::{AccountHash, AccountRead, SigninState, Source};
use clauseo_protocol::{AgentId, ErrorCode, SignIn};
use clauseo_settings::SystemFiles;

use super::fakes::{HostCall, Rig, rig_identity};
use super::switching::shown;
use super::{claude, codex};
use crate::bridge::lock;
use crate::setup::identity::{
    Identities, KEYCHAIN_STALE, Seen, Verdict, check, on_state, poll_once,
};
use crate::setup::keys::{KeyNote, KeyNotes};
use crate::setup::login_check::LOGIN_FILE;
use crate::setup::refusal::follow_keys;
use crate::test_support::{Scratch, block_on};

const ONE: &str = "lawyer@firm.example";
const OTHER: &str = "someone-else@elsewhere.example";

pub(super) fn signed_in(email: &str, source: Source) -> SigninState {
    SigninState::SignedIn {
        email: Some(email.to_owned()),
        plan: Some("plus".to_owned()),
        source,
        at: UNIX_EPOCH,
    }
}

/// A rig whose agent homes and data folder are in `scratch`, the record read from there.
pub(super) struct IdRig {
    pub(super) rig: Rig,
    pub(super) scratch: Scratch,
}

impl IdRig {
    pub(super) fn new(test: &str) -> Self {
        Self::with(test, |identity| identity)
    }

    /// [`IdRig::new`], its identity check changed by `with` (the Keychain fallback, say), the
    /// record as `scratch` holds it then.
    pub(super) fn with(test: &str, with: impl FnOnce(Identities) -> Identities) -> Self {
        let dirs = [
            "homes/claude-acp",
            "homes/codex-acp",
            "data/agents",
            "lawyer",
        ];
        let scratch = Scratch::folder(test, &dirs);
        Self::over(scratch, with)
    }

    /// The rig over `scratch` as it is: a start of the app after an earlier one.
    pub(super) fn over(scratch: Scratch, with: impl FnOnce(Identities) -> Identities) -> Self {
        let identity = Identities::load(
            &scratch.path().join("data"),
            Arc::new(SystemFiles),
            Some(scratch.path().join("lawyer")),
            [&claude(), &codex()],
        );
        let rig = rig_identity(&scratch.path().join("homes"), with(identity));
        Self { rig, scratch }
    }

    /// Codex's login file in its home, a test's dummy that holds no credential.
    pub(super) fn codex_login(&self) -> PathBuf {
        self.scratch.path().join("homes/codex-acp").join(LOGIN_FILE)
    }

    /// Another file put in place of Codex's login file: another file id, as a swap makes.
    pub(super) fn replace_codex_login(&self, tag: &str) {
        let other = self.scratch.file(&format!("homes/codex-acp/{tag}"), b"{}");
        replace(&other, &self.codex_login());
    }

    /// A sign-in through Clauseo that ends `SignedIn` with `email`, as the pump takes it:
    /// the identity first (it records), then the composer.
    pub(super) fn signs_in(&self, agent: &AgentId, email: &str) -> Seen {
        let backend = &self.rig.backend;
        backend.wiring.identity.expect(agent);
        let state = signed_in(email, Source::Own);
        let seen = block_on(on_state(backend, agent, &state));
        backend.compose.signin(agent, state);
        seen
    }

    /// `secs` seconds past now on the composer's clock, which the checks are timed by.
    pub(super) fn at(&self, secs: u64) -> Duration {
        self.rig.backend.compose.monotonic() + Duration::from_secs(secs)
    }

    /// One look at the login files at [`IdRig::at`] `secs`.
    pub(super) fn look(&self, secs: u64) {
        block_on(poll_once(&self.rig.backend, self.at(secs)));
    }

    pub(super) fn reads(&self) -> usize {
        let calls = self.rig.calls();
        calls
            .iter()
            .filter(|(name, _)| *name == "read_account")
            .count()
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "test set-up swaps a dummy login file in its scratch folder"
)]
fn replace(from: &Path, to: &Path) {
    std::fs::rename(from, to).unwrap();
}

fn refused(agent: &AgentId, code: Option<ErrorCode>) -> HostCall {
    HostCall::SetLaunchRefused(agent.clone(), code)
}

fn account_changed(sign_in: Option<&SignIn>) -> bool {
    matches!(
        sign_in,
        Some(SignIn::Failed {
            code: ErrorCode::SigninAccountChanged,
            ..
        })
    )
}

#[test]
fn a_swap_while_sessions_run_stops_the_agents_and_says_sign_in_again() {
    let id = IdRig::new("identity-swap");
    let (rig, agent) = (&id.rig, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    assert_eq!(id.signs_in(&agent, ONE), Seen::SignedIn);
    // The first look takes the stamps the verdict holds for; sessions run on it meanwhile.
    id.look(0);
    assert_eq!(id.reads(), 0);
    // A prompt-injected agent in matter A puts another account's login in place.
    id.replace_codex_login("swapped");
    rig.agents.accounts.names(&agent, OTHER);
    rig.journal();
    id.look(2);
    assert_eq!(
        rig.backend.wiring.identity.verdict(&agent),
        Verdict::Changed
    );
    assert_eq!(
        rig.journal(),
        [
            "host.set_launch_refused",
            "agents.read_account",
            "host.set_launch_refused",
            "host.stop_agents",
        ],
        "checking first, then the read; the refusal before the stop, so nothing launches between"
    );
    let calls = rig.host.calls();
    assert_eq!(
        calls[calls.len() - 3..],
        [
            refused(&agent, Some(ErrorCode::AgentIdentityChecking)),
            refused(&agent, Some(ErrorCode::SigninAccountChanged)),
            HostCall::StopAgents(agent.clone()),
        ]
    );
    assert!(account_changed(shown(rig, &agent).as_ref()));
    // It stays: the old account coming back does not lift it, and nothing checks it.
    rig.agents.accounts.names(&agent, ONE);
    id.replace_codex_login("back");
    id.look(4);
    assert_eq!(
        rig.backend.wiring.identity.verdict(&agent),
        Verdict::Changed
    );
    assert_eq!(id.reads(), 1);
    // A new sign-in through Clauseo records the new account and lifts it.
    assert_eq!(id.signs_in(&agent, OTHER), Seen::SignedIn);
    assert_eq!(rig.host.calls().last(), Some(&refused(&agent, None)));
    assert!(!account_changed(shown(rig, &agent).as_ref()));
}

#[test]
fn a_status_check_naming_another_account_is_a_change_never_a_new_sign_in() {
    let id = IdRig::new("identity-status-swap");
    let (rig, agent) = (&id.rig, codex());
    id.signs_in(&agent, ONE);
    rig.host.calls();
    // S18's status check (a window shows Plans) after the swap: before R17 the pump took this
    // as a new sign-in and restarted the agents on it.
    rig.pumped(async {
        rig.s18_sends(&agent, signed_in(OTHER, Source::Own));
    });
    let calls = rig.host.calls();
    assert!(
        calls.contains(&HostCall::StopAgents(agent.clone())),
        "{calls:?}"
    );
    assert!(
        !calls.contains(&HostCall::RestartAgents(agent.clone())),
        "{calls:?}"
    );
    assert!(account_changed(shown(rig, &agent).as_ref()));
    assert_eq!(
        rig.backend.wiring.identity.recorded(&agent),
        AccountHash::of(ONE)
    );
}

#[test]
fn a_login_file_change_with_the_same_account_checks_then_lifts_and_stops_nothing() {
    let id = IdRig::new("identity-same");
    let (rig, agent) = (&id.rig, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&agent, ONE);
    id.look(0);
    // A token refresh rewrites the file: the same account.
    id.replace_codex_login("refreshed");
    rig.agents.accounts.names(&agent, ONE);
    id.look(2);
    assert_eq!(id.reads(), 1);
    assert_eq!(
        rig.host.calls(),
        [
            refused(&agent, Some(ErrorCode::AgentIdentityChecking)),
            refused(&agent, None),
        ]
    );
    assert_eq!(rig.backend.wiring.identity.verdict(&agent), Verdict::Same);
    // Unchanged since: no other read.
    id.look(4);
    assert_eq!(id.reads(), 1);
}

#[test]
fn the_identity_refusal_never_lifts_or_hides_a_policy_refusal() {
    let id = IdRig::new("identity-coexist");
    let (rig, agent) = (&id.rig, codex());
    id.signs_in(&agent, ONE);
    lock(&rig.backend.wiring.applied)
        .blocked
        .insert(agent.clone());
    rig.agents.accounts.names(&agent, ONE);
    assert_eq!(block_on(check(&rig.backend, &agent)), Verdict::Same);
    rig.agents.accounts.names(&agent, OTHER);
    block_on(check(&rig.backend, &agent));
    assert_eq!(
        rig.backend.wiring.identity.verdict(&agent),
        Verdict::Changed
    );
    // The policy's refusal is the one in force throughout; the identity's waits under it.
    assert_eq!(
        rig.host.calls()[0],
        refused(&agent, Some(ErrorCode::SigninPolicyUnreadable))
    );
    assert!(!rig.host.calls().contains(&refused(&agent, None)));
}

#[test]
fn a_status_that_fails_or_names_no_account_is_unknown_and_checked_again_later() {
    let id = IdRig::new("identity-unknown");
    let (rig, agent) = (&id.rig, codex());
    id.signs_in(&agent, ONE);
    for read in [AccountRead::Failed, AccountRead::NoAccount] {
        rig.agents.accounts.answers(&agent, read.clone());
        assert_eq!(
            block_on(check(&rig.backend, &agent)),
            Verdict::Unknown,
            "{read:?}"
        );
    }
    assert!(
        rig.host
            .calls()
            .contains(&refused(&agent, Some(ErrorCode::AgentIdentityUnknown)))
    );
    assert!(
        !rig.host
            .calls()
            .contains(&HostCall::StopAgents(agent.clone()))
    );
    // Checked again by itself once `UNKNOWN_AGAIN` has passed, and lifted when it answers.
    rig.agents.accounts.names(&agent, ONE);
    id.look(0);
    id.look(60);
    assert_eq!(rig.backend.wiring.identity.verdict(&agent), Verdict::Same);
    assert_eq!(rig.host.calls().last(), Some(&refused(&agent, None)));
}

#[test]
fn a_status_that_never_answers_is_unknown_at_the_bound() {
    let id = IdRig::with("identity-bound", |mut identity| {
        identity.bound = Duration::from_millis(50);
        identity
    });
    let (rig, agent) = (&id.rig, codex());
    id.signs_in(&agent, ONE);
    rig.agents.accounts.stall(true);
    assert_eq!(block_on(check(&rig.backend, &agent)), Verdict::Unknown);
}

#[test]
fn a_fresh_verdict_runs_no_status_at_launch() {
    let id = IdRig::new("identity-fresh");
    let (rig, agent) = (&id.rig, codex());
    id.scratch.file("homes/codex-acp/auth.json", b"{}");
    id.signs_in(&agent, ONE);
    for second in 0..10 {
        id.look(second * 2);
    }
    assert_eq!(id.reads(), 0, "no status run while nothing changed");
    assert_eq!(
        rig.host.calls(),
        [],
        "no refusal: a launch goes ahead at once"
    );
    assert_eq!(crate::setup::refusal::wanted(&rig.backend, &agent), None);
}

#[test]
fn a_keychain_only_login_is_checked_again_before_a_launch_once_stale() {
    let id = IdRig::with("identity-keychain", |identity| {
        identity.with_keychain_fallback(true)
    });
    let (rig, agent) = (&id.rig, claude());
    // No store file: Claude's login is in the Keychain.
    id.signs_in(&agent, ONE);
    id.look(0);
    let stale = KEYCHAIN_STALE.as_secs();
    id.look(stale - 1);
    assert_eq!(
        (id.reads(), rig.host.calls()),
        (0, Vec::new()),
        "fresh: nothing"
    );
    id.look(stale);
    assert_eq!(id.reads(), 0, "stale: due, nothing run until a launch asks");
    assert_eq!(
        rig.host.calls(),
        [refused(&agent, Some(ErrorCode::AgentIdentityChecking))]
    );
    // A launch meets the refusal; the host's launch retry asks for the check.
    rig.agents.accounts.names(&agent, ONE);
    assert_eq!(block_on(check(&rig.backend, &agent)), Verdict::Same);
    assert_eq!(id.reads(), 1);
    assert_eq!(rig.host.calls().last(), Some(&refused(&agent, None)));
}

#[test]
fn the_launch_retry_reads_the_key_for_key_unreadable_and_asks_the_check_for_identity() {
    let id = IdRig::new("identity-dispatch");
    let backend = &id.rig.backend;
    let notes = KeyNotes::new();
    let retry = notes.retry_hook();
    let mut asks = backend.wiring.identity.take_asks().unwrap();
    retry(&codex(), ErrorCode::AgentIdentityChecking);
    retry(&claude(), ErrorCode::AgentKeyUnreadable);
    retry(&claude(), ErrorCode::SigninPolicyUnreadable);
    let mut receiver = notes.take().unwrap();
    let mut sent = Vec::new();
    while let Ok(note) = receiver.try_recv() {
        sent.push(note);
    }
    assert_eq!(
        sent,
        [
            KeyNote::Retry(codex(), ErrorCode::AgentIdentityChecking),
            KeyNote::Retry(claude(), ErrorCode::AgentKeyUnreadable),
            KeyNote::Retry(claude(), ErrorCode::SigninPolicyUnreadable),
        ]
    );
    let (feed, notes_in) = futures::channel::mpsc::unbounded();
    for note in sent {
        feed.unbounded_send(note).unwrap();
    }
    drop(feed);
    block_on(follow_keys(backend, notes_in));
    assert_eq!(
        asks.try_recv().ok(),
        Some(codex()),
        "identity: the check is asked for"
    );
    assert!(asks.try_recv().is_err(), "the other codes ask for no check");
    // `agent.key_unreadable`'s read again: key_state.rs's
    // `follow_keys_sets_the_refusal_from_each_note_and_a_retry_reads_the_key_again`.
}

#[test]
fn signing_in_again_to_the_same_account_after_a_change_restarts_the_held_questions() {
    let id = IdRig::new("identity-again");
    let (rig, agent) = (&id.rig, codex());
    id.signs_in(&agent, ONE);
    lock(&rig.agents.signin).insert(agent.clone(), signed_in(ONE, Source::Own));
    // The watcher finds another account; the lawyer then signs in again, to the first one.
    rig.agents.accounts.names(&agent, OTHER);
    block_on(check(&rig.backend, &agent));
    assert_eq!(
        rig.backend.wiring.identity.verdict(&agent),
        Verdict::Changed
    );
    lock(&rig.agents.answers).insert("sign_in", signed_in(ONE, Source::Own));
    let sent = super::fakes::command("start_sign_in", &agent, &serde_json::json!({}));
    rig.pumped(async { rig.setup().run(super::MAIN, sent).await.unwrap() });
    assert_eq!(rig.backend.wiring.identity.verdict(&agent), Verdict::Same);
    let calls = rig.host.calls();
    let stop = calls
        .iter()
        .position(|call| *call == HostCall::StopAgents(agent.clone()));
    let restart = calls
        .iter()
        .position(|call| *call == HostCall::RestartAgents(agent.clone()));
    assert!(
        stop < restart && restart.is_some(),
        "the sign-in restarts after the stop: {calls:?}"
    );
    assert_eq!(
        calls
            .iter()
            .rev()
            .find(|call| matches!(call, HostCall::SetLaunchRefused(..))),
        Some(&refused(&agent, None))
    );
    assert!(!account_changed(shown(rig, &agent).as_ref()));
}
