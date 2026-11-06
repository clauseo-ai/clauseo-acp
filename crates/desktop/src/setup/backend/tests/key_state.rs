//! A stored key that cannot be read, as the windows see it and as the refusal follows it (the
//! final security review's L4, the desktop review's item 1 and its added tests): it shows
//! `Failed { agent.key_unreadable }`, never an API key line, and bars no sign-in; a restart sets
//! the refusal before anything launches; a late read loses to a newer key; clearing the key
//! lifts the refusal; `follow_keys` sets the refusal from each note and reads the key again on
//! a launch's retry.

use std::time::{Duration, UNIX_EPOCH};

use clauseo_agents::{SigninState, Source};
use clauseo_process::{AsyncIoTimer, Timer as _};
use clauseo_protocol::{CommandOutcome, ErrorCode, Secret, SignIn, SignInMethod};
use futures::future::{Either, select};
use serde_json::json;
use zeroize::Zeroizing;

use super::fakes::{HostCall, Rig, command, rig_keys, rig_on};
use super::key_reads::{KEY, Never, stored};
use super::switching::shown;
use super::{MAIN, SETTINGS, claude, codex};
use crate::setup::KeyWrite;
use crate::setup::keys::{KEYS_LOAD_BOUND, KeyCache, KeyNotes};
use crate::setup::leave::restart_if_usable;
use crate::setup::refusal::{follow_keys, sync};
use crate::test_support::{block_on, within};

const NEW_KEY: &str = "fakeAnthropicKey4567";
const OTHER_KEY: &str = "fakeAnthropicKey8901";

const OUT: SigninState = SigninState::SignedOut { found: None };

/// A rig whose stored keys could not be read (every read refused, as at a denied prompt),
/// both agents signed out, and the windows told as `start.rs` tells them.
fn unreadable() -> Rig {
    let keychain = stored();
    keychain.fail_reads(true);
    let rig = rig_on(true, keychain);
    for agent in [claude(), codex()] {
        rig.s18_was(&agent, OUT);
        rig.backend.compose.show_key(&agent, &rig.backend.keys);
    }
    rig
}

fn key_unreadable(shown: Option<&SignIn>) -> bool {
    matches!(
        shown,
        Some(SignIn::Failed {
            code: ErrorCode::AgentKeyUnreadable,
            ..
        })
    )
}

fn api_key(shown: Option<&SignIn>) -> bool {
    matches!(
        shown,
        Some(SignIn::SignedIn {
            method: SignInMethod::ApiKey,
            ..
        })
    )
}

fn key_of(keys: &KeyCache) -> Option<String> {
    keys.get(&claude()).map(|key| String::clone(&key))
}

#[test]
fn an_unreadable_key_shows_key_unreadable_never_an_api_key_and_bars_no_sign_in() {
    let rig = unreadable();
    let line = shown(&rig, &claude());
    assert!(key_unreadable(line.as_ref()), "{line:?}");
    // Codex has no key stored, but a read that failed cannot tell: refused all the same.
    assert!(key_unreadable(shown(&rig, &codex()).as_ref()));
    assert_eq!(rig.backend.compose.sign_in_bar(&claude()), None);
    let start = command("start_sign_in", &claude(), &json!({}));
    assert_eq!(
        block_on(rig.setup().run(MAIN, start)),
        Ok(CommandOutcome::Done),
        "never signin.key_in_use"
    );
    assert_eq!(rig.calls(), [("sign_in", claude())], "the sign-in runs");
    // Signed in with the plan: launches stay refused until a read succeeds, and the line says so.
    let plan = SigninState::SignedIn {
        email: None,
        plan: Some("max".to_owned()),
        source: Source::Own,
        at: UNIX_EPOCH,
    };
    rig.s18_was(&claude(), plan);
    assert!(key_unreadable(shown(&rig, &claude()).as_ref()));
    assert!(
        rig.backend.keys.has(&claude()),
        "still stored, for the launches"
    );
    rig.keychain.fail_reads(false);
    block_on(rig.backend.keys.read_again(&claude())).unwrap();
    rig.backend.compose.show_key(&claude(), &rig.backend.keys);
    assert!(
        api_key(shown(&rig, &claude()).as_ref()),
        "read: the key's line"
    );
}

#[test]
fn a_restart_sets_the_key_unreadable_refusal_before_anything_launches() {
    // The key turned unreadable and its note is not taken yet: the host was told nothing.
    let rig = unreadable();
    assert_eq!(rig.host.calls(), []);
    block_on(restart_if_usable(&rig.backend, &claude()));
    assert_eq!(
        rig.host.calls(),
        [
            HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyUnreadable)),
            HostCall::RestartAgents(claude()),
        ]
    );
}

#[test]
fn a_late_read_loses_to_a_newer_set_key() {
    let keychain = stored();
    keychain.fail_reads(true);
    let keys = KeyCache::load_all(keychain.clone(), [&claude()]);
    assert!(keys.unreadable(&claude()));
    // A launch's read again waits on a prompt nobody has answered yet.
    keychain.fail_reads(false);
    keychain.hang_reads(true);
    let late = keys.read_again(&claude());
    let key = Secret::new(Zeroizing::new(NEW_KEY.to_owned())).unwrap();
    block_on(keys.set(&claude(), key, KeyWrite::Replace, |_| {})).unwrap();
    // The item changes behind Clauseo's back: the late read would take that one in.
    keychain.store(&claude(), OTHER_KEY).unwrap();
    keychain.hang_reads(false);
    block_on(late).unwrap();
    assert_eq!(
        key_of(&keys).as_deref(),
        Some(NEW_KEY),
        "the newer key stands"
    );
    assert!(!keys.unreadable(&claude()));
}

#[test]
fn clear_key_lifts_the_key_unreadable_refusal() {
    let rig = unreadable();
    block_on(sync(&rig.backend, &claude()));
    let refused = HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyUnreadable));
    assert_eq!(rig.host.calls(), [refused]);
    let clear = command("clear_agent_key", &claude(), &json!({}));
    assert_eq!(
        block_on(rig.setup().run(SETTINGS, clear)),
        Ok(CommandOutcome::Done)
    );
    assert_eq!(
        rig.host.calls()[1..],
        [
            HostCall::SetLaunchRefused(claude(), None),
            HostCall::RestartAgents(claude()),
            HostCall::LoginChanged(claude()),
        ],
        "lifted before the restart launches"
    );
    assert_eq!(
        shown(&rig, &claude()),
        Some(SignIn::SignedOut { found: None })
    );
    rig.keychain.fail_reads(false);
    assert_eq!(rig.keychain.load(&claude()).unwrap(), None, "cleared");
}

/// Waits, on the clock, until `done` holds.
async fn wait_for(done: impl Fn() -> bool) {
    while !done() {
        AsyncIoTimer.after(Duration::from_millis(10)).await;
    }
}

#[test]
fn follow_keys_sets_the_refusal_from_each_note_and_a_retry_reads_the_key_again() {
    let keychain = stored();
    keychain.fail_reads(true);
    let notes = KeyNotes::new();
    let agents = [&claude(), &codex()];
    let keys = KeyCache::load_bounded(
        keychain.clone(),
        agents,
        notes.sender(),
        &Never,
        KEYS_LOAD_BOUND,
    );
    let rig = rig_keys(keychain.clone(), keys);
    let receiver = notes.take().unwrap();
    let retry = notes.retry_hook();
    let host = &rig.host;
    let told = |code| {
        let call = HostCall::SetLaunchRefused(claude(), code);
        move || host.calls().contains(&call)
    };
    let drive = async {
        // The load's notes: both reads failed.
        wait_for(told(Some(ErrorCode::AgentKeyUnreadable))).await;
        assert!(key_unreadable(shown(&rig, &claude()).as_ref()));
        // The prompt is allowed; a launch meets the refusal and asks for a read again.
        keychain.fail_reads(false);
        retry(&claude());
        wait_for(told(None)).await;
    };
    let follow = follow_keys(&rig.backend, receiver);
    within("follow_keys lifts the refusal", async {
        match select(std::pin::pin!(follow), std::pin::pin!(drive)).await {
            Either::Left(((), _)) => panic!("the notes ended"),
            Either::Right(((), _)) => {}
        }
    });
    assert_eq!(key_of(&rig.backend.keys).as_deref(), Some(KEY));
    assert!(api_key(shown(&rig, &claude()).as_ref()));
}
