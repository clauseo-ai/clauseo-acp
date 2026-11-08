//! The backend answers while the keychain is still being read (`keys/loaded.rs`): its cell is
//! filled before the keys' first load, so a command that reads no key answers at once, one
//! that reads them answers once the load is over (and with what the keys say), and the host's
//! own calls never wait on either. The keychain here is a memory one whose reads hang, as a
//! slow Credential Manager read would, past the load's 10 s bound, until the test lets them go.

use std::sync::Arc;
use std::sync::mpsc;
use std::time::Duration;

use clauseo_agents::keys::Keychain;
use clauseo_protocol::{CommandOutcome, ErrorCode, HostError, HostQuery, QueryResult};
use serde_json::{Value, json};

use super::fakes::{Rig, command, rig_loading};
use super::key_reads::{KEY, Never, stored};
use super::{MAIN, SETTINGS, claude};
use crate::bridge::Bridge;
use crate::host_start::HostCell;
use crate::setup::keys::{
    FirstLoad, KEYS_LOAD_BOUND, KeyCache, KeyMarker, KeyNotes, KeysLoaded, LoadDone, LoadOutcome,
};
use crate::setup::{Setup, SetupBackend};
use crate::test_support::{Scratch, block_on};

/// Well under any keychain read: what "at once" means here.
const AT_ONCE: Duration = Duration::from_millis(100);

/// A rig over `keychain` with Claude's key stored and every read hanging: the load started,
/// not over, and what ends it.
fn loading() -> (Rig, FirstLoad, LoadDone) {
    let keychain = stored();
    keychain.hang_reads(true);
    let notes = KeyNotes::new();
    let agents = [&claude(), &super::codex()];
    let (keys, load) = KeyCache::start_load(
        keychain.clone(),
        agents,
        KeyMarker::unkept(),
        notes.sender(),
    );
    let (loaded, done) = KeysLoaded::pending();
    (rig_loading(keychain, keys, loaded), load, done)
}

/// What the setup thread does once the reads answer: waits for them, tells `Compose` each key,
/// and ends the load.
fn finish_load(rig: &Rig, load: FirstLoad, done: LoadDone) {
    rig.keychain.hang_reads(false);
    let outcome = load.wait(&rig.backend.keys, &Never, KEYS_LOAD_BOUND);
    assert_eq!(outcome, LoadOutcome::Read);
    for agent in [claude(), super::codex()] {
        rig.backend.compose.show_key(&agent, &rig.backend.keys);
    }
    done.finish();
}

/// What a command sent apart answers.
type Answer = Result<CommandOutcome, HostError>;

/// Sends `name` for Claude from Settings on a thread of its own: its answer, once it comes.
fn send_apart(rig: &Rig, name: &'static str, args: Value) -> mpsc::Receiver<Answer> {
    let setup = rig.setup();
    let (tx, answers) = mpsc::channel();
    std::thread::spawn(move || {
        let sent = command(name, &claude(), &args);
        tx.send(block_on(setup.run(SETTINGS, sent))).unwrap();
    });
    answers
}

/// Claude's key as the cache binds it, and as the keychain holds it.
fn claude_key(rig: &Rig) -> (Option<String>, Option<String>) {
    rig.keychain.hang_reads(false);
    let bound = rig.backend.keys.get(&claude()).map(|key| key.to_string());
    let held = rig
        .keychain
        .load(&claude())
        .unwrap()
        .map(|key| key.to_string());
    (bound, held)
}

#[test]
fn a_command_that_reads_no_key_answers_at_once_while_the_keychain_hangs() {
    let (rig, load, done) = loading();
    let setup = rig.setup();
    for (name, label) in [("install_agent", MAIN), ("open_sign_in_page", MAIN)] {
        let began = crate::open::now();
        let answer = block_on(setup.run(label, command(name, &claude(), &json!({}))));
        let took = began.elapsed();
        assert_eq!(answer, Ok(CommandOutcome::Done), "{name}");
        assert!(took < AT_ONCE, "{name} took {took:?}");
    }
    assert!(rig.calls().iter().any(|(call, _)| *call == "install"));
    finish_load(&rig, load, done);
}

#[test]
fn a_sign_in_sent_while_the_keys_load_answers_after_them_with_what_they_say() {
    let (rig, load, done) = loading();
    let setup = rig.setup();
    let (tx, answers) = mpsc::channel();
    let sender = std::thread::spawn(move || {
        let start = command("start_sign_in", &claude(), &json!({}));
        tx.send(block_on(setup.run(MAIN, start))).unwrap();
    });
    // The keys have not read: the start waits for them, and no sign-in starts.
    let waiting = answers.recv_timeout(Duration::from_millis(300));
    assert_eq!(waiting, Err(mpsc::RecvTimeoutError::Timeout));
    assert!(!rig.calls().iter().any(|(call, _)| *call == "sign_in"));
    finish_load(&rig, load, done);
    // Claude's stored key bars the sign-in: had the start not waited, it would have run.
    let answer = answers.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(answer.unwrap_err().code(), ErrorCode::SigninKeyInUse);
    assert!(!rig.calls().iter().any(|(call, _)| *call == "sign_in"));
    sender.join().unwrap();
}

#[test]
fn a_host_query_answers_while_a_sign_in_waits_for_the_keys() {
    let scratch = Scratch::new("keys-loading-query", &[]);
    let host = scratch.start(Vec::new());
    let (rig, load, done) = loading();
    let backend = Arc::clone(&rig.backend) as Arc<dyn SetupBackend>;
    let bridge = Arc::new(Bridge::with_setup(
        HostCell::ready(host.host.clone()),
        Setup::new(backend),
    ));
    let (tx, answers) = mpsc::channel();
    let waiting = Arc::clone(&bridge);
    let sender = std::thread::spawn(move || {
        let start = command("start_sign_in", &claude(), &json!({}));
        tx.send(block_on(waiting.setup(MAIN, start))).unwrap();
    });
    assert_eq!(
        answers.recv_timeout(Duration::from_millis(100)),
        Err(mpsc::RecvTimeoutError::Timeout)
    );
    let began = crate::open::now();
    let settings = block_on(bridge.query(MAIN, HostQuery::GetSettings));
    let took = began.elapsed();
    assert!(
        matches!(settings, Ok(QueryResult::Settings(_))),
        "{settings:?}"
    );
    assert!(took < AT_ONCE * 5, "the query took {took:?}");
    finish_load(&rig, load, done);
    assert!(answers.recv_timeout(Duration::from_secs(5)).is_ok());
    sender.join().unwrap();
    // The bridge's windows go before the host stops.
    drop(bridge);
    drop(host);
}

#[test]
fn a_keychain_that_never_answers_ends_the_load_at_its_bound() {
    let keychain = Keychain::memory();
    keychain.hang_reads(true);
    let notes = KeyNotes::new();
    let (keys, load) = KeyCache::start_load(
        keychain.clone(),
        [&claude()],
        KeyMarker::unkept(),
        notes.sender(),
    );
    let outcome = load.wait(&keys, &super::key_reads::Passed, KEYS_LOAD_BOUND);
    assert_eq!(outcome, LoadOutcome::BoundPassed);
    assert_eq!(outcome.as_str(), "bound_passed");
    keychain.hang_reads(false);
}

#[test]
fn a_key_cleared_while_the_keys_load_waits_for_them_and_stays_cleared() {
    let (rig, load, done) = loading();
    let answers = send_apart(&rig, "clear_agent_key", json!({}));
    // The clear waits: the stored key is still there for the load to read.
    let waiting = answers.recv_timeout(Duration::from_millis(300));
    assert_eq!(waiting, Err(mpsc::RecvTimeoutError::Timeout));
    finish_load(&rig, load, done);
    let answer = answers.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(answer, Ok(CommandOutcome::Done));
    // The load landed first, so the clear is the last word: no key bound, none stored.
    assert_eq!(claude_key(&rig), (None, None));
}

#[test]
fn a_key_set_while_the_keys_load_waits_for_them_and_wins() {
    const NEW_KEY: &str = "sk-ant-api03-NEW-KEY-0002";
    let (rig, load, done) = loading();
    let answers = send_apart(&rig, "set_agent_key", json!({ "key": NEW_KEY }));
    let waiting = answers.recv_timeout(Duration::from_millis(300));
    assert_eq!(waiting, Err(mpsc::RecvTimeoutError::Timeout));
    finish_load(&rig, load, done);
    let answer = answers.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(answer, Ok(CommandOutcome::Done));
    // The loaded key does not land over the new one.
    let new = Some(NEW_KEY.to_owned());
    assert_eq!(claude_key(&rig), (new.clone(), new));
}

#[test]
fn a_sign_out_while_the_keys_load_answers_at_once_and_leaves_the_stored_key() {
    let (rig, load, done) = loading();
    let began = crate::open::now();
    let answer = block_on(
        rig.setup()
            .run(SETTINGS, command("sign_out", &claude(), &json!({}))),
    );
    let took = began.elapsed();
    assert_eq!(answer, Ok(CommandOutcome::Done));
    assert!(took < AT_ONCE, "sign_out took {took:?}");
    assert!(rig.calls().iter().any(|(call, _)| *call == "sign_out"));
    finish_load(&rig, load, done);
    // A sign-out ends the CLI's login, never the stored key: the same as after the load.
    let key = Some(KEY.to_owned());
    assert_eq!(claude_key(&rig), (key.clone(), key));
}
