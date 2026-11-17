//! The "key stored" marker in the key cache (decision record 0048, R20 P5b A4): with a keychain
//! that never answers by the bound, or that fails the read, an agent the marker does not mark
//! launches without a key and one it marks is refused with `agent.key_unreadable`; a corrupt
//! marker marks every agent. A
//! save sets the marker before the keychain write and is refused when it cannot; a clear removes
//! the key, then the marker; each read that answers brings the marker in step.

use std::collections::BTreeSet;
use std::io;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, UNIX_EPOCH};

use clauseo_agents::SigninState;
use clauseo_agents::keys::Keychain;
use clauseo_protocol::{AgentId, ErrorCode, Secret, SignIn};
use clauseo_settings::{Files, SystemFiles};
use futures::StreamExt as _;
use zeroize::Zeroizing;

use super::fakes::{HostCall, rig};
use super::key_reads::{KEY, Never, Passed, stored};
use super::{claude, codex};
use crate::bridge::lock;
use crate::setup::KeyWrite;
use crate::setup::compose::{Facts, compose};
use crate::setup::keys::marker::{self, Marked};
use crate::setup::keys::{KEYS_LOAD_BOUND, KeyCache, KeyMarker, KeyNote, KeyNotes};
use crate::setup::refusal::settle_after_load;
use crate::test_support::{Scratch, block_on};

/// A [`Scratch`] for a test that releases a hung read: that read writes the marker on its own
/// thread, and the write can land after the scratch's removal and leave its temporary file
/// behind. Its drop pauses first, so the write lands before the scratch removes the folder.
struct Late(Scratch);

impl Late {
    fn folder(test: &str, dirs: &[&str]) -> Self {
        Self(Scratch::folder(test, dirs))
    }
}

impl std::ops::Deref for Late {
    type Target = Scratch;

    fn deref(&self) -> &Scratch {
        &self.0
    }
}

impl Drop for Late {
    #[expect(
        clippy::disallowed_methods,
        reason = "the write runs on a thread the injected clock cannot see, so only a real pause lets it land"
    )]
    fn drop(&mut self) {
        std::thread::sleep(Duration::from_millis(50));
    }
}

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
    settled_with(keys, &BTreeSet::new())
}

/// [`settled`], with the agents in `firm` under a firm plan (an endpoint or a provider).
fn settled_with(keys: &KeyCache, firm: &BTreeSet<AgentId>) -> Vec<HostCall> {
    let rig = rig(true);
    let agents = [&claude(), &codex()];
    block_on(settle_after_load(
        &*rig.host,
        keys,
        agents,
        &BTreeSet::new(),
        firm,
    ));
    rig.host.calls()
}

fn secret(text: &str) -> Secret {
    Secret::new(Zeroizing::new(text.to_owned())).unwrap()
}

#[test]
fn a_silent_keychain_refuses_the_marked_agent_and_launches_the_unmarked_one_without_a_key() {
    let scratch = Late::folder("key-marker-silent", &[]);
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
    let scratch = Late::folder("key-marker-fresh", &[]);
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
    let scratch = Late::folder("key-marker-corrupt-load", &["agents"]);
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

#[test]
fn a_late_read_that_answers_a_key_marks_an_unmarked_agent() {
    let scratch = Late::folder("key-marker-late-key", &[]);
    // Codex's key is stored, but no marker marks Codex: its read is silent at the bound.
    let keychain = Keychain::memory();
    keychain.store(&codex(), KEY).unwrap();
    keychain.hang_reads(true);
    let notes = KeyNotes::new();
    let keys = KeyCache::load_bounded(
        keychain.clone(),
        [&claude(), &codex()],
        KeyMarker::new(scratch.path(), [&claude(), &codex()]),
        notes.sender(),
        &Passed,
        KEYS_LOAD_BOUND,
    );
    assert!(!keys.unreadable(&codex()) && !keys.has(&codex()));
    assert_eq!(marked(&scratch), Marked::Only(BTreeSet::new()));
    // At the bound it counts as a failed read: refused only under a firm plan.
    assert!(keys.read_failed(&codex()) && !keys.unreadable_for(&codex(), false));
    // Claude's read, unmarked too, counts as failed as well: one note each, in either order.
    let mut receiver = notes.take().unwrap();
    let mut late = [block_on(receiver.next()), block_on(receiver.next())];
    late.sort_by_key(|note| format!("{note:?}"));
    assert_eq!(
        late,
        [Some(KeyNote::Read(claude())), Some(KeyNote::Read(codex()))]
    );
    // The late read answers with the key: it is taken in, and the marker is set before the
    // change is noted (Claude's late answer, no key, is noted too, in either order).
    keychain.hang_reads(false);
    loop {
        let note = block_on(receiver.next());
        assert!(note.is_some(), "the notes ended");
        if note == Some(KeyNote::Read(codex())) {
            break;
        }
    }
    assert!(!keys.read_failed(&codex()));
    assert_eq!(keys.get(&codex()).as_deref().map(String::as_str), Some(KEY));
    assert_eq!(marked(&scratch), Marked::Only([codex()].into()));
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), None),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
}

#[test]
fn a_keychain_error_for_an_unmarked_agent_launches_it_without_a_key() {
    let scratch = Scratch::folder("key-marker-error", &[]);
    // Codex's key is stored and no marker marks it; the keychain refuses the read (an error,
    // not a timeout). Claude's read is refused too, though it holds no key. Neither is marked,
    // so each is taken as a read that timed out: no key Clauseo stored, no refusal (M-d).
    let keychain = Keychain::memory();
    keychain.store(&codex(), KEY).unwrap();
    keychain.fail_reads(true);
    let keys = load(
        keychain.clone(),
        KeyMarker::new(scratch.path(), [&claude(), &codex()]),
        &Never,
    );
    assert!(!keys.unreadable(&codex()) && !keys.has(&codex()));
    assert!(!keys.unreadable(&claude()) && !keys.has(&claude()));
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), None),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
    assert_eq!(marked(&scratch), Marked::Only(BTreeSet::new()));
    keychain.fail_reads(false);
}

#[test]
fn a_keychain_error_for_a_marked_agent_is_still_refused_as_key_denied() {
    let scratch = Scratch::folder("key-marker-error-marked", &[]);
    // Claude's key was stored through Clauseo (marked); the keychain now refuses every read.
    // A firm's key must never be silently skipped: Claude stays refused. Codex is unmarked.
    let keychain = Keychain::memory();
    keychain.store(&claude(), KEY).unwrap();
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    marker.set(&claude()).unwrap();
    keychain.fail_reads(true);
    let keys = load(keychain.clone(), marker, &Never);
    assert!(keys.unreadable(&claude()) && keys.has(&claude()));
    assert_eq!(keys.get(&claude()), None);
    assert!(!keys.unreadable(&codex()) && !keys.has(&codex()));
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyDenied)),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
    assert_eq!(marked(&scratch), Marked::Only([claude()].into()));
    keychain.fail_reads(false);
}

#[test]
fn a_lost_marker_with_a_healthy_keychain_changes_nothing() {
    let scratch = Scratch::folder("key-marker-lost", &[]);
    let keychain = Keychain::memory();
    keychain.store(&claude(), KEY).unwrap();
    // Claude's key is stored, but the marker file is gone: a folder with no marker file stands
    // in for one that was lost.
    let marker = KeyMarker::new(scratch.path(), [&claude(), &codex()]);
    assert_eq!(marked(&scratch), Marked::Only(BTreeSet::new()));
    // The read answers: the stored key is read and used, and the marker is set again.
    let keys = load(keychain, marker, &Never);
    assert!(!keys.unreadable(&claude()));
    assert_eq!(
        keys.get(&claude()).as_deref().map(String::as_str),
        Some(KEY)
    );
    assert_eq!(
        settled(&keys),
        [
            HostCall::SetLaunchRefused(claude(), None),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
    assert_eq!(marked(&scratch), Marked::Only([claude()].into()));
}

#[test]
fn no_keychain_and_nothing_marked_composes_to_sign_in() {
    let scratch = Scratch::folder("key-marker-no-keychain", &[]);
    // M-d: no login keychain, so every read fails; no agent is marked. The status check's
    // answer (`SignedOut`) is what the card shows: Sign in, not "Sign-in didn't finish.".
    let keychain = Keychain::memory();
    keychain.fail_reads(true);
    let keys = load(
        keychain.clone(),
        KeyMarker::new(scratch.path(), [&claude(), &codex()]),
        &Never,
    );
    let s18 = SigninState::SignedOut { found: None };
    for agent in [claude(), codex()] {
        let facts = Facts {
            key: keys.in_memory(&agent).then_some(UNIX_EPOCH),
            unreadable: keys.unreadable(&agent).then_some(UNIX_EPOCH),
            read_failed: keys.read_failed(&agent).then_some(UNIX_EPOCH),
            ..Facts::default()
        };
        assert!(
            matches!(
                compose(&agent, Some(&s18), facts),
                Some(SignIn::SignedOut { .. })
            ),
            "{agent:?} shows Sign in"
        );
    }
    keychain.fail_reads(false);
}

#[test]
fn a_keychain_error_under_a_firm_plan_refuses_an_unmarked_agent_with_key_denied() {
    let scratch = Scratch::folder("key-marker-error-firm", &[]);
    // Neither agent is marked and every read fails, but Claude has a firm endpoint or provider:
    // its launch must not go keyless, so a lawyer's own sign-in token never reaches the firm's
    // gateway, and "Firm" on screen stays true. Codex, with no firm plan, launches as above.
    let keychain = Keychain::memory();
    keychain.fail_reads(true);
    let keys = load(
        keychain.clone(),
        KeyMarker::new(scratch.path(), [&claude(), &codex()]),
        &Never,
    );
    assert!(!keys.unreadable(&claude()) && keys.read_failed(&claude()));
    assert!(keys.unreadable_for(&claude(), true));
    assert!(!keys.unreadable_for(&codex(), false));
    assert_eq!(
        settled_with(&keys, &BTreeSet::from([claude()])),
        [
            HostCall::SetLaunchRefused(claude(), Some(ErrorCode::AgentKeyDenied)),
            HostCall::SetLaunchRefused(codex(), None),
        ]
    );
    // The card says so: the firm line gives way to the unreadable key, as for a marked agent.
    let facts = Facts {
        firm: Some(UNIX_EPOCH),
        read_failed: keys.read_failed(&claude()).then_some(UNIX_EPOCH),
        denied: keys.denied(&claude()),
        ..Facts::default()
    };
    let s18 = SigninState::SignedOut { found: None };
    assert!(matches!(
        compose(&claude(), Some(&s18), facts),
        Some(SignIn::Failed {
            code: ErrorCode::AgentKeyDenied,
            ..
        })
    ));
    let managed = Facts {
        managed: Some(UNIX_EPOCH),
        firm: None,
        ..facts
    };
    assert!(matches!(
        compose(&claude(), Some(&s18), managed),
        Some(SignIn::Failed {
            code: ErrorCode::AgentKeyDenied,
            ..
        })
    ));
    keychain.fail_reads(false);
}
