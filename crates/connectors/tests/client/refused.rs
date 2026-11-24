//! A keychain that refuses to read the item (macOS's errSecInteractionNotAllowed, a platform
//! failure, which is not "no item"): the credential is `Unreadable`, never signed out, and
//! sign-out, remove, sign-in and a new key are refused before the item is deleted or replaced.

use clauseo_connectors::{CredentialKind, CredentialStatus, Error, SignInError, StoreError};

use crate::support::*;

/// A signed-in rig restarted while the keychain refuses reads: the new handle and the item.
fn refused_after_restart(
    rig: &Rig,
) -> (
    clauseo_connectors::Connectors,
    clauseo_connectors::ConnectorHandle,
    Vec<u8>,
) {
    sign_in(rig);
    let stored = rig.store.peek("case-law").unwrap();
    let (connectors, handle) = rig.restart();
    rig.store.refuse_reads(true);
    rig.fake.clear_requests();
    (connectors, handle, stored)
}

fn is_unreadable(err: &StoreError) -> bool {
    matches!(err, StoreError::Refused(_))
}

#[test]
fn a_refused_read_is_unreadable_and_read_again_once_allowed() {
    let rig = rig();
    let (_connectors, handle, stored) = refused_after_restart(&rig);
    let state = state_file(&rig);
    let reads = rig.store.reads();
    assert_eq!(handle.status(), CredentialStatus::Unreadable);
    assert_eq!(handle.status(), CredentialStatus::Unreadable);
    assert_eq!(
        rig.store.reads(),
        reads + 2,
        "each look reads the item again"
    );
    assert_eq!(rig.store.peek("case-law").unwrap(), stored);
    assert_eq!(rig.store.deletes(), 0);
    assert_eq!(state_file(&rig), state, "nothing recorded");
    rig.store.refuse_reads(false);
    assert_eq!(
        handle.status(),
        CredentialStatus::SignedIn(CredentialKind::OAuth)
    );
}

#[test]
fn a_bearer_while_refused_fails_and_deletes_nothing() {
    let rig = rig();
    let (_connectors, handle, stored) = refused_after_restart(&rig);
    let state = state_file(&rig);
    assert!(matches!(
        handle.bearer(),
        Err(clauseo_connectors::AuthError::Store(_))
    ));
    assert_eq!(rig.store.peek("case-law").unwrap(), stored);
    assert_eq!(rig.store.deletes(), 0);
    assert_eq!(state_file(&rig), state);
}

#[test]
fn sign_out_while_refused_is_refused_and_deletes_nothing() {
    let rig = rig();
    let (_connectors, handle, stored) = refused_after_restart(&rig);
    let state = state_file(&rig);
    let err = handle.sign_out().unwrap_err();
    assert!(
        matches!(&err, Error::Store(e) if is_unreadable(e)),
        "{err:?}"
    );
    assert_eq!(rig.store.peek("case-law").unwrap(), stored);
    assert_eq!(rig.store.deletes(), 0);
    assert_eq!(rig.fake.requests_to("/revoke").len(), 0);
    assert_eq!(state_file(&rig), state);
    assert_eq!(handle.status(), CredentialStatus::Unreadable);
    // Allowed now: the sign-out reads the item, revokes its token and deletes it.
    rig.store.refuse_reads(false);
    handle.sign_out().unwrap();
    assert_eq!(rig.fake.requests_to("/revoke").len(), 1);
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(handle.status(), CredentialStatus::SignedOut);
}

#[test]
fn remove_while_refused_is_refused_and_forgets_nothing() {
    let rig = rig();
    let (_connectors, handle, stored) = refused_after_restart(&rig);
    let state = state_file(&rig);
    let err = handle.remove().unwrap_err();
    assert!(
        matches!(&err, Error::Store(e) if is_unreadable(e)),
        "{err:?}"
    );
    assert_eq!(rig.store.peek("case-law").unwrap(), stored);
    assert_eq!(rig.store.deletes(), 0);
    assert_eq!(state_file(&rig), state, "the connector stays added");
}

#[test]
fn sign_in_while_refused_is_refused_before_the_browser_opens() {
    let rig = rig();
    let (_connectors, handle, stored) = refused_after_restart(&rig);
    let state = state_file(&rig);
    let writes = rig.store.writes();
    let err = handle.begin_sign_in(None).unwrap_err();
    assert!(
        matches!(&err, SignInError::Store(e) if is_unreadable(e)),
        "{err:?}"
    );
    assert_eq!(handle.listeners_bound_for_test(), 0);
    assert_eq!(rig.fake.requests().len(), 0);
    assert_eq!(rig.store.writes(), writes);
    assert_eq!(rig.store.peek("case-law").unwrap(), stored);
    assert_eq!(state_file(&rig), state);
}

/// The read is refused only once the browser's sign-in is under way: the item is not replaced,
/// and the new tokens, never used, are revoked.
#[test]
fn a_read_refused_at_the_store_replaces_nothing_and_revokes_the_new_token() {
    let rig = rig();
    sign_in(&rig);
    let stored = rig.store.peek("case-law").unwrap();
    let (_connectors, handle) = rig.restart();
    let pending = handle.begin_sign_in(None).unwrap();
    rig.store.refuse_reads(true);
    rig.fake.clear_requests();
    let writes = rig.store.writes();
    let (_page, result) = complete(&rig.fake, pending);
    let err = result.unwrap_err();
    assert!(
        matches!(&err, SignInError::Store(e) if is_unreadable(e)),
        "{err:?}"
    );
    assert_eq!(rig.store.writes(), writes, "nothing written");
    assert_eq!(rig.store.peek("case-law").unwrap(), stored);
    let old = serde_json::from_slice::<serde_json::Value>(&stored).unwrap()["refresh_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let revoked: Vec<String> = rig
        .fake
        .requests_to("/revoke")
        .iter()
        .filter_map(|request| {
            request
                .form()
                .into_iter()
                .find(|(name, _)| name == "token")
                .map(|(_, token)| token)
        })
        .collect();
    assert_eq!(revoked.len(), 1, "the new token only");
    assert_ne!(revoked[0], old, "the stored token is never revoked here");
}

#[test]
fn a_new_key_while_refused_is_refused_before_any_request() {
    let rig = rig();
    let (_connectors, handle, stored) = refused_after_restart(&rig);
    let key = rig.fake.new_api_key();
    let writes = rig.store.writes();
    let err = handle.add_api_key(&key).unwrap_err();
    assert!(
        matches!(&err, Error::Store(e) if is_unreadable(e)),
        "{err:?}"
    );
    assert_eq!(rig.fake.requests().len(), 0);
    assert_eq!(rig.store.writes(), writes);
    assert_eq!(rig.store.peek("case-law").unwrap(), stored);
}

/// "No item" keeps today's answers: signed out, and a key goes ahead.
#[test]
fn a_missing_item_is_still_signed_out() {
    let rig = rig();
    sign_in(&rig);
    let (_connectors, handle) = rig.restart();
    // Deleted behind this handle's back (another instance signed out).
    let (_other_connectors, other) = rig.restart();
    other.sign_out().unwrap();
    assert_eq!(handle.status(), CredentialStatus::SignedOut);
    let key = rig.fake.new_api_key();
    handle.add_api_key(&key).unwrap();
    assert_eq!(
        handle.status(),
        CredentialStatus::SignedIn(CredentialKind::ApiKey)
    );
}
