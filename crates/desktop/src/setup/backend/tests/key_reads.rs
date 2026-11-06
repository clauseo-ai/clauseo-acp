//! The bounded first load of the stored keys and each agent's refusal after it (decision
//! record 0048, R20 P5b A4): a stored key whose read does not answer by the bound, or fails,
//! refuses the agent with `agent.key_unreadable` and never reads as "no key"; no stored key,
//! or one read fine, lifts the refusal; a later read that succeeds (the slow one answering, or
//! a read again when a launch met the refusal) lifts it.

use std::future::Future;
use std::time::Duration;

use clauseo_agents::keys::Keychain;
use clauseo_protocol::ErrorCode;
use futures::StreamExt as _;

use super::fakes::{HostCall, rig, rig_on};
use super::{claude, codex};
use crate::setup::keys::{KEYS_LOAD_BOUND, KeyCache, KeyMarker, KeyNote, KeyNotes};
use crate::setup::refusal::{settle_after_load, sync, sync_all};
use crate::test_support::block_on;

pub(super) const KEY: &str = "fakeAnthropicKey0123";

/// A timer whose waits end at once: the bound has passed.
#[derive(Clone)]
pub(super) struct Passed;

impl clauseo_process::Timer for Passed {
    fn after(&self, _duration: Duration) -> impl Future<Output = ()> + Send {
        std::future::ready(())
    }
}

/// A timer whose waits never end: the load waits for every read.
#[derive(Clone)]
pub(super) struct Never;

impl clauseo_process::Timer for Never {
    fn after(&self, _duration: Duration) -> impl Future<Output = ()> + Send {
        std::future::pending()
    }
}

/// A memory keychain with Claude's key stored.
pub(super) fn stored() -> Keychain {
    let keychain = Keychain::memory();
    keychain.store(&claude(), KEY).unwrap();
    keychain
}

/// The load the setup thread makes, over `keychain` and `timer`, with its notes.
fn load(keychain: Keychain, timer: &impl clauseo_process::Timer) -> (KeyCache, KeyNotes) {
    let notes = KeyNotes::new();
    let keys = KeyCache::load_bounded(
        keychain,
        [&claude(), &codex()],
        KeyMarker::unkept(),
        notes.sender(),
        timer,
        KEYS_LOAD_BOUND,
    );
    (keys, notes)
}

/// The refusals the host is told once the load is over.
fn settled(keys: &KeyCache) -> Vec<HostCall> {
    let rig = rig(true);
    block_on(settle_after_load(
        &*rig.host,
        keys,
        [&claude(), &codex()],
        &std::collections::BTreeSet::new(),
    ));
    rig.host.calls()
}

#[test]
fn a_stored_key_whose_read_times_out_is_key_unreadable_and_lifts_once_the_read_answers() {
    let keychain = stored();
    keychain.hang_reads(true);
    let (keys, notes) = load(keychain.clone(), &Passed);
    assert!(keys.unreadable(&claude()));
    assert!(keys.has(&claude()), "an unreadable key is never no key");
    assert_eq!(keys.get(&claude()), None);
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyUnreadable)),
            // Codex's read timed out as well: its key may be stored, so it is refused too.
            HostCall::SetLaunchRefused(codex(), Some(ErrorCode::AgentKeyUnreadable)),
        ]
    );
    let mut receiver = notes.take().unwrap();
    // A launch's retry while the slow read still waits starts no second read.
    drop(keys.read_again(&claude()));
    // The prompt is answered: the slow reads land, and each mark's end is noted.
    keychain.hang_reads(false);
    let mut read = vec![
        block_on(receiver.next()).unwrap(),
        block_on(receiver.next()).unwrap(),
        block_on(receiver.next()).unwrap(),
        block_on(receiver.next()).unwrap(),
    ];
    read.sort_by_key(|note| format!("{note:?}"));
    assert_eq!(
        read,
        [
            KeyNote::Read(claude()),
            KeyNote::Read(claude()),
            KeyNote::Read(codex()),
            KeyNote::Read(codex()),
        ],
        "marked at the bound, then ended"
    );
    assert!(!keys.unreadable(&claude()));
    assert_eq!(
        keys.get(&claude()).as_deref().map(String::as_str),
        Some(KEY)
    );
    assert!(!keys.has(&codex()), "no key stored for Codex");
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), None),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
}

#[test]
fn a_refused_read_is_key_unreadable_never_no_key() {
    let keychain = stored();
    keychain.fail_reads(true);
    let (keys, _notes) = load(keychain, &Never);
    assert!(keys.unreadable(&claude()));
    assert!(keys.has(&claude()));
    assert_eq!(keys.get(&claude()), None);
    assert_eq!(
        settled(&keys)[0],
        HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyUnreadable))
    );
}

#[test]
fn no_stored_key_lifts_and_a_key_read_fine_lifts_with_the_key() {
    let (keys, _notes) = load(stored(), &Never);
    assert!(!keys.unreadable(&claude()) && !keys.unreadable(&codex()));
    // Claude launches with its key; Codex, with none stored, launches without one.
    assert_eq!(
        keys.get(&claude()).as_deref().map(String::as_str),
        Some(KEY)
    );
    assert_eq!(keys.get(&codex()), None);
    assert!(!keys.has(&codex()));
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), None),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
}

#[test]
fn a_read_again_that_succeeds_lifts_the_refusal() {
    let keychain = stored();
    keychain.fail_reads(true);
    let rig = rig_on(true, keychain.clone());
    block_on(sync_all(&rig.backend));
    // Every read failed, so Codex's (no key stored, which the failure hid) is refused too.
    assert_eq!(
        rig.host.calls(),
        [
            HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyUnreadable)),
            HostCall::SetLaunchRefused(codex(), Some(ErrorCode::AgentKeyUnreadable)),
        ]
    );
    // Still refused: a read again fails, and the refusal stays as it was.
    block_on(rig.backend.keys.read_again(&claude())).unwrap();
    assert!(rig.backend.keys.unreadable(&claude()));
    block_on(sync(&rig.backend, &claude()));
    assert_eq!(rig.host.calls().len(), 2, "nothing new to tell the host");
    // The lawyer allows it; the next launch's read again succeeds and lifts the refusal.
    keychain.fail_reads(false);
    block_on(rig.backend.keys.read_again(&claude())).unwrap();
    assert!(!rig.backend.keys.unreadable(&claude()));
    block_on(sync(&rig.backend, &claude()));
    assert_eq!(
        rig.host.calls()[2..],
        [HostCall::SetLaunchRefused(claude(), None)]
    );
    assert_eq!(
        rig.backend
            .keys
            .get(&claude())
            .as_deref()
            .map(String::as_str),
        Some(KEY)
    );
}

#[test]
fn a_key_set_while_unreadable_ends_the_mark() {
    let keychain = stored();
    keychain.fail_reads(true);
    let rig = rig_on(true, keychain);
    assert!(rig.backend.keys.unreadable(&claude()));
    let key =
        clauseo_protocol::Secret::new(zeroize::Zeroizing::new("fakeAnthropicKey4567".to_owned()))
            .unwrap();
    block_on(
        rig.backend
            .keys
            .set(&claude(), key, crate::setup::KeyWrite::Replace, |_| {}),
    )
    .unwrap();
    assert!(!rig.backend.keys.unreadable(&claude()));
    block_on(sync(&rig.backend, &claude()));
    assert!(
        rig.host.calls().is_empty(),
        "never refused: nothing to lift"
    );
}
