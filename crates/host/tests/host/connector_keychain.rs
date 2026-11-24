//! A keychain that refuses to read case law's saved sign-in (macOS's errSecInteractionNotAllowed
//! before the lawyer presses Allow, a platform failure, not "no item"): the host never shows it
//! signed out, the list's checks keep reading it, Sign out, Remove, Sign in and a new key are
//! refused with `connector.keychain_refused` before the item is touched, and `KeychainReadAgain`
//! reads it again once allowed. A delete or a write the keychain refuses keeps the old item and
//! the state file, `connector.keychain_refused` for the command.

use std::time::Duration;

use clauseo_protocol::{
    CommandOutcome, ConnectorErrorKind, ConnectorStatus, ConnectorsState, ErrorCode, HostCommand,
};

use crate::addons::addons;
use clauseo_connectors::StoreError;

use crate::connector_rig::{CASE_LAW, Rig, Start, add, add_key, remove, sign_out, status};
use crate::support::{Dir, wait};

/// The state's line while the saved sign-in can't be read.
const UNREADABLE: &str = "the saved sign-in could not be read from the keychain";

/// The status the host shows while the keychain refuses the read.
fn unreadable(state: &ConnectorsState) -> bool {
    matches!(
        status(state),
        Some(ConnectorStatus::Error {
            message,
            kind: Some(ConnectorErrorKind::SignInFailed),
        }) if message == UNREADABLE
    )
}

/// A rig signed in on an earlier run, started with the keychain refusing reads, past the state
/// its start-up read publishes.
fn refused(name: &str) -> (Dir, Rig) {
    let dir = Dir::new(name, &[]);
    let mut rig = Rig::start_refused(&dir);
    let state = rig.next_state("the start-up read");
    assert!(unreadable(&state), "{state:?}");
    (dir, rig)
}

/// Polls until `done`, for at most 10 s of real time.
fn until(what: &str, done: impl Fn() -> bool) {
    for _ in 0..2_000 {
        if done() {
            return;
        }
        wait("a pause", async_io::Timer::after(Duration::from_millis(5)));
    }
    panic!("{what} never happened");
}

/// Refused at the start: never signed out, left out of the list, and a read of the list still
/// starts a check, which reads the item again. Allowed, `KeychainReadAgain` reads it: signed in.
#[test]
fn a_refused_read_is_never_signed_out_and_reads_again_once_allowed() {
    let (_dir, mut rig) = refused("connector_keychain_start");
    let reads = rig.store.reads();
    let items = addons(&rig.host);
    assert!(!items.iter().any(|row| row.id == CASE_LAW), "{items:?}");
    // The read of the list started its check: it reads the item again, refused again.
    until("the check's read", || rig.store.reads() > reads);
    assert!(rig.store.peek(CASE_LAW).is_some());
    assert_eq!(rig.store.deletes(), 0);

    rig.store.refuse_reads(false);
    assert_eq!(
        rig.host.dispatch(HostCommand::KeychainReadAgain {}),
        Ok(CommandOutcome::Done)
    );
    let state = rig.state_until("signed in", |state| {
        matches!(status(state), Some(ConnectorStatus::SignedIn { .. }))
    });
    assert!(!unreadable(&state));
    rig.finish();
}

/// Sign out and Remove while refused: `connector.keychain_refused`, the item and the state file
/// as they were, nothing revoked.
#[test]
fn sign_out_and_remove_while_refused_delete_nothing() {
    let (_dir, mut rig) = refused("connector_keychain_sign_out");
    let state_file = std::fs::read(rig.scratch().join("connectors").join("state.json")).unwrap();
    let item = rig.store.peek(CASE_LAW).unwrap();
    for command in [sign_out(), remove()] {
        let refused = rig.dispatch(command).unwrap_err();
        assert_eq!(
            refused.code(),
            ErrorCode::ConnectorKeychainRefused,
            "{refused}"
        );
    }
    assert_eq!(rig.store.peek(CASE_LAW).unwrap(), item);
    assert_eq!(rig.store.deletes(), 0);
    assert_eq!(rig.fake.requests_to("/revoke").len(), 0);
    let after = std::fs::read(rig.scratch().join("connectors").join("state.json")).unwrap();
    assert_eq!(after, state_file);
    rig.finish();
}

/// Sign in and a new key while refused: `connector.keychain_refused` before the browser opens
/// or the key is checked, the item as it was.
#[test]
fn sign_in_and_a_new_key_while_refused_overwrite_nothing() {
    let (_dir, mut rig) = refused("connector_keychain_sign_in");
    let item = rig.store.peek(CASE_LAW).unwrap();
    let writes = rig.store.writes();
    let account_reads = rig.account_reads();
    let refused = rig.dispatch(add()).unwrap_err();
    assert_eq!(
        refused.code(),
        ErrorCode::ConnectorKeychainRefused,
        "{refused}"
    );
    assert!(rig.pages.try_recv().is_err(), "the sign-in page opened");
    let key = rig.fake.new_api_key();
    rig.keys.push(key.clone());
    let refused = rig.dispatch(add_key(&key)).unwrap_err();
    assert_eq!(
        refused.code(),
        ErrorCode::ConnectorKeychainRefused,
        "{refused}"
    );
    assert_eq!(rig.account_reads(), account_reads, "the key was checked");
    assert_eq!(rig.store.writes(), writes);
    assert_eq!(rig.store.peek(CASE_LAW).unwrap(), item);
    rig.finish();
}

/// Signed in, then the keychain refuses deletes (errSecNoAccessForItem on another build's item)
/// and reads: Sign out revokes but its delete is refused, so `connector.keychain_refused`, the
/// item and the state file as they were, and the connector shows it could not be read.
#[test]
fn a_refused_delete_at_sign_out_is_keychain_refused_and_keeps_the_item() {
    let dir = Dir::new("connector_keychain_delete", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let state_file = std::fs::read(rig.scratch().join("connectors").join("state.json")).unwrap();
    let item = rig.store.peek(CASE_LAW).unwrap();
    rig.store
        .fail_deletes_with(Some(StoreError::NoAccess("no storage access".to_owned())));
    rig.store
        .fail_reads_with(Some(StoreError::Platform("a platform failure".to_owned())));
    let refused = rig.dispatch(sign_out()).unwrap_err();
    assert_eq!(
        refused.code(),
        ErrorCode::ConnectorKeychainRefused,
        "{refused}"
    );
    assert_eq!(rig.store.peek(CASE_LAW).unwrap(), item);
    let after = std::fs::read(rig.scratch().join("connectors").join("state.json")).unwrap();
    assert_eq!(after, state_file);
    rig.state_until("unreadable", unreadable);
    rig.finish();
}

/// Signed in, then the keychain refuses writes (errSecDuplicateItem over another build's item):
/// a new key is `connector.keychain_refused`, the old item and the state file as they were.
#[test]
fn a_refused_write_of_a_new_key_is_keychain_refused_and_keeps_the_item() {
    let dir = Dir::new("connector_keychain_write", &[]);
    let mut rig = Rig::start(&dir, Start { signed_in: true });
    let state_file = std::fs::read(rig.scratch().join("connectors").join("state.json")).unwrap();
    let item = rig.store.peek(CASE_LAW).unwrap();
    rig.store
        .fail_writes_with(Some(StoreError::Platform("a platform failure".to_owned())));
    let key = rig.fake.new_api_key();
    rig.keys.push(key.clone());
    let refused = rig.dispatch(add_key(&key)).unwrap_err();
    assert_eq!(
        refused.code(),
        ErrorCode::ConnectorKeychainRefused,
        "{refused}"
    );
    assert_eq!(rig.store.peek(CASE_LAW).unwrap(), item);
    let after = std::fs::read(rig.scratch().join("connectors").join("state.json")).unwrap();
    assert_eq!(after, state_file);
    rig.finish();
}
