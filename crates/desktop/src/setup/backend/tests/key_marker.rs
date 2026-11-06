//! The "key stored" marker in the key cache (decision record 0048, R20 P5b A4): with a keychain
//! that never answers by the bound, an agent the marker does not mark launches without a key and
//! one it marks is refused with `agent.key_unreadable`; a corrupt marker marks every agent. A
//! save sets the marker before the keychain write and is refused when it cannot; a clear removes
//! the key, then the marker; each read that answers brings the marker in step.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use clauseo_agents::keys::Keychain;
use clauseo_protocol::{ErrorCode, Secret};
use clauseo_settings::{Files, SystemFiles};
use zeroize::Zeroizing;

use super::fakes::{HostCall, rig};
use super::key_reads::{KEY, Never, Passed, stored};
use super::{claude, codex};
use crate::bridge::lock;
use crate::setup::KeyWrite;
use crate::setup::keys::marker::{self, Marked};
use crate::setup::keys::{KEYS_LOAD_BOUND, KeyCache, KeyMarker, KeyNotes};
use crate::setup::refusal::settle_after_load;
use crate::test_support::{Scratch, block_on};

/// The real file system, its writes failing while `fail` is set; each write records whether
/// the keychain held Claude's key when it was made.
struct Watched {
    fail: AtomicBool,
    keychain: Keychain,
    key_there: Mutex<Vec<bool>>,
}

impl Watched {
    fn new(keychain: &Keychain) -> Arc<Self> {
        Arc::new(Self {
            fail: AtomicBool::new(false),
            keychain: keychain.clone(),
            key_there: Mutex::default(),
        })
    }
}

impl Files for Watched {
    fn read(&self, path: &Path) -> io::Result<Option<Vec<u8>>> {
        SystemFiles.read(path)
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        let there = matches!(self.keychain.load(&claude()), Ok(Some(_)));
        lock(&self.key_there).push(there);
        SystemFiles.write_atomic(path, bytes)
    }
}

/// The marker in `scratch`'s folder, through `files`.
fn marker_in(scratch: &Scratch, files: Arc<dyn Files + Send + Sync>) -> KeyMarker {
    KeyMarker::with_files(marker::path(scratch.path()), files, [&claude(), &codex()])
}

/// What the marker in `scratch`'s folder marks, read afresh.
fn marked(scratch: &Scratch) -> Marked {
    KeyMarker::new(scratch.path(), [&claude(), &codex()]).marked()
}

/// The setup thread's load over `keychain`, `marker` and `timer`.
fn load(keychain: Keychain, marker: KeyMarker, timer: &impl clauseo_process::Timer) -> KeyCache {
    let notes = KeyNotes::new();
    let agents = [&claude(), &codex()];
    KeyCache::load_bounded(
        keychain,
        agents,
        marker,
        notes.sender(),
        timer,
        KEYS_LOAD_BOUND,
    )
}

/// The refusals the host is told once the load is over.
fn settled(keys: &KeyCache) -> Vec<HostCall> {
    let rig = rig(true);
    let agents = [&claude(), &codex()];
    block_on(settle_after_load(
        &*rig.host,
        keys,
        agents,
        &BTreeSet::new(),
    ));
    rig.host.calls()
}

fn secret(text: &str) -> Secret {
    Secret::new(Zeroizing::new(text.to_owned())).unwrap()
}

#[test]
fn a_silent_keychain_refuses_the_marked_agent_and_launches_the_unmarked_one_without_a_key() {
    let scratch = Scratch::folder("key-marker-silent", &[]);
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    marker.set(&claude()).unwrap();
    let keychain = stored();
    keychain.hang_reads(true);
    let keys = load(keychain.clone(), marker, &Passed);
    assert!(keys.unreadable(&claude()) && keys.has(&claude()));
    assert!(!keys.unreadable(&codex()) && !keys.has(&codex()));
    assert_eq!(keys.get(&codex()), None, "Codex launches without a key");
    assert_eq!(
        settled(&keys),
        [
            // Marked: no launch, so no process, until a read answers.
            HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyUnreadable)),
            // Unmarked: `agent.keys_loading` is lifted and nothing refuses it.
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
    keychain.hang_reads(false);
}

#[test]
fn a_silent_keychain_with_no_marker_file_refuses_no_agent() {
    let scratch = Scratch::folder("key-marker-fresh", &[]);
    let keychain = Keychain::memory();
    keychain.hang_reads(true);
    let keys = load(
        keychain.clone(),
        KeyMarker::new(scratch.path(), [&claude()]),
        &Passed,
    );
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), None),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
    keychain.hang_reads(false);
}

#[test]
fn a_silent_keychain_with_a_corrupt_marker_refuses_every_agent() {
    let scratch = Scratch::folder("key-marker-corrupt-load", &["agents"]);
    scratch.file("agents/key-stored.json", b"[\"claude-acp\"");
    let keychain = Keychain::memory();
    keychain.hang_reads(true);
    let keys = load(
        keychain.clone(),
        KeyMarker::new(scratch.path(), [&claude()]),
        &Passed,
    );
    let refused = Some(ErrorCode::AgentKeyUnreadable);
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), refused),
            HostCall::SetLaunchRefused(codex(), refused),
        ]
    );
    keychain.hang_reads(false);
}

#[test]
fn a_save_sets_the_marker_before_the_keychain_write_and_the_file_holds_no_key() {
    let scratch = Scratch::folder("key-marker-save", &[]);
    let keychain = Keychain::memory();
    let files = Watched::new(&keychain);
    let keys = load(
        keychain.clone(),
        marker_in(&scratch, Arc::clone(&files) as _),
        &Never,
    );
    block_on(keys.set(&claude(), secret(KEY), KeyWrite::Replace, |_| {})).unwrap();
    assert_eq!(*lock(&files.key_there), [false], "the marker went first");
    assert!(matches!(keychain.load(&claude()), Ok(Some(_))));
    let text = std::fs::read_to_string(marker::path(scratch.path())).unwrap();
    assert!(!text.contains(KEY), "{text}");
    assert!(
        !text.contains(&clauseo_agents::keys::account(&claude())),
        "{text}"
    );
    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(value, serde_json::json!({ "agents": ["claude-acp"] }));
}

#[test]
fn a_failing_keychain_write_leaves_the_marker_on() {
    let scratch = Scratch::folder("key-marker-keychain-fails", &[]);
    // No keychain: every store fails.
    let keys = load(
        Keychain::none(),
        KeyMarker::new(scratch.path(), [&claude()]),
        &Never,
    );
    let stored = block_on(keys.set(&claude(), secret(KEY), KeyWrite::Replace, |_| {}));
    assert_eq!(stored.unwrap_err().code(), ErrorCode::Internal);
    assert_eq!(marked(&scratch), Marked::Only([claude()].into()));
}

#[test]
fn a_marker_that_cannot_be_written_refuses_the_save_and_stores_nothing() {
    let scratch = Scratch::folder("key-marker-write-fails", &[]);
    let keychain = Keychain::memory();
    let files = Watched::new(&keychain);
    files.fail.store(true, Ordering::SeqCst);
    let keys = load(keychain.clone(), marker_in(&scratch, files), &Never);
    let then = Arc::new(AtomicBool::new(false));
    let told = Arc::clone(&then);
    let write = KeyWrite::Replace;
    let stored = block_on(keys.set(&claude(), secret(KEY), write, move |_| {
        told.store(true, Ordering::SeqCst);
    }));
    let error = stored.unwrap_err();
    assert_eq!(error.code(), ErrorCode::Internal);
    assert!(!error.message().contains(KEY));
    assert_eq!(keychain.load(&claude()).unwrap(), None, "nothing stored");
    assert!(!keys.in_memory(&claude()) && !then.load(Ordering::SeqCst));
    assert_eq!(marked(&scratch), Marked::Only(BTreeSet::new()));
}

#[test]
fn a_clear_removes_the_key_then_the_marker_and_a_failing_marker_clear_leaves_it_on() {
    let scratch = Scratch::folder("key-marker-clear", &[]);
    let keychain = Keychain::memory();
    let files = Watched::new(&keychain);
    let keys = load(
        keychain.clone(),
        marker_in(&scratch, Arc::clone(&files) as _),
        &Never,
    );
    for agent in [claude(), codex()] {
        block_on(keys.set(&agent, secret(KEY), KeyWrite::Replace, |_| {})).unwrap();
    }
    lock(&files.key_there).clear();
    block_on(keys.clear(&claude(), |_| {})).unwrap();
    assert_eq!(*lock(&files.key_there), [false], "the key went first");
    assert_eq!(marked(&scratch), Marked::Only([codex()].into()));
    // The marker cannot be cleared: the key goes all the same, and the marker stays on.
    files.fail.store(true, Ordering::SeqCst);
    block_on(keys.clear(&codex(), |_| {})).unwrap();
    assert_eq!(keychain.load(&codex()).unwrap(), None);
    assert_eq!(marked(&scratch), Marked::Only([codex()].into()));
}

#[test]
fn a_read_that_answers_sets_a_missing_marker_and_clears_a_stale_one() {
    let scratch = Scratch::folder("key-marker-reconcile", &[]);
    // Codex is marked, but no key of its is stored (removed outside Clauseo); Claude's key is
    // stored, but not marked (stored by a build without the marker).
    KeyMarker::new(scratch.path(), [&codex()])
        .set(&codex())
        .unwrap();
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    let keys = load(stored(), marker, &Never);
    assert!(keys.in_memory(&claude()));
    assert_eq!(marked(&scratch), Marked::Only([claude()].into()));
    // A read that fails leaves the marker as it was.
    let keychain = stored();
    keychain.fail_reads(true);
    let _keys = load(
        keychain,
        KeyMarker::new(scratch.path(), [&claude()]),
        &Never,
    );
    assert_eq!(marked(&scratch), Marked::Only([claude()].into()));
}
