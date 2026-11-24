//! The keychain's refusals as a real Mac gives them, through the fake keychain: errSecAuthFailed
//! (-25293: a read it may not ask about, or a locked keychain) and errSecDuplicateItem (-25299:
//! a write over another build's item) arrive as keyring's platform failure, and
//! errSecNoAccessForItem (-25244: a delete of another build's item) as no storage access. Each
//! is a refusal, never "no item" and never success: a read is `Unreadable`, no automatic delete
//! runs while the last read was refused, and a write or a delete refused leaves the old item
//! and the state file as they were, `StoreError::Refused`.

use std::time::Duration;

use clauseo_connectors::{AuthError, CredentialStatus, Error, SignInError, StoreError};

use crate::support::*;

/// Each refusal, by the OS code it stands for, as `store/keyring.rs`'s `map` gives it.
fn refusals() -> [(&'static str, StoreError); 3] {
    [
        (
            "-25293 errSecAuthFailed",
            StoreError::Platform("a platform failure".to_owned()),
        ),
        (
            "-25299 errSecDuplicateItem",
            StoreError::Platform("a platform failure".to_owned()),
        ),
        (
            "-25244 errSecNoAccessForItem",
            StoreError::NoAccess("no storage access".to_owned()),
        ),
    ]
}

fn refused(err: &StoreError) -> bool {
    matches!(err, StoreError::Refused(_))
}

/// Each code on a read: `Unreadable`, a sign-out and a sign-in refused, the item kept.
#[test]
fn each_refusal_on_a_read_is_unreadable_never_signed_out() {
    for (code, error) in refusals() {
        let rig = rig();
        sign_in(&rig);
        let stored = rig.store.peek("case-law").unwrap();
        let state = state_file(&rig);
        let (_connectors, handle) = rig.restart();
        rig.store.fail_reads_with(Some(error));
        assert_eq!(handle.status(), CredentialStatus::Unreadable, "{code}");
        assert!(
            matches!(handle.bearer(), Err(AuthError::Store(_))),
            "{code}"
        );
        let out = handle.sign_out().unwrap_err();
        assert!(
            matches!(&out, Error::Store(e) if refused(e)),
            "{code}: {out:?}"
        );
        let begin = handle.begin_sign_in(None).unwrap_err();
        assert!(
            matches!(&begin, SignInError::Store(e) if refused(e)),
            "{code}"
        );
        assert_eq!(rig.store.peek("case-law").unwrap(), stored, "{code}");
        assert_eq!(rig.store.deletes(), 0, "{code}");
        assert_eq!(state_file(&rig), state, "{code}");
    }
}

/// The automatic delete after `invalid_grant` does not run while the re-read before the refresh
/// was refused: the item may hold a newer chain. The item and the state file stay, and the
/// credential reads `Unreadable` until a read works.
#[test]
fn invalid_grant_after_a_refused_reread_deletes_nothing() {
    for (code, error) in refusals() {
        let rig = manual_rig();
        sign_in(&rig);
        rig.handle.bearer().unwrap();
        let stored = rig.store.peek("case-law").unwrap();
        rig.fake.revoke_all();
        rig.store.fail_reads_with(Some(error));
        rig.clock().advance(Duration::from_secs(3600));
        let out = rig.handle.bearer();
        assert!(
            matches!(&out, Err(AuthError::Store(e)) if refused(e)),
            "{code}: {out:?}"
        );
        assert_eq!(rig.store.deletes(), 0, "{code}");
        assert_eq!(rig.store.peek("case-law").unwrap(), stored, "{code}");
        assert!(state_file(&rig).contains(r#""ended":null"#), "{code}");
        assert_eq!(rig.handle.status(), CredentialStatus::Unreadable, "{code}");
        // Readable again: the item's dead token is refused, and now it goes.
        rig.store.fail_reads_with(None);
        assert_eq!(rig.handle.bearer(), Err(AuthError::SignedOut), "{code}");
        assert!(rig.store.peek("case-law").is_none(), "{code}");
    }
}

/// An item that does not parse is deleted, but a delete the keychain refuses records nothing:
/// the state file stays and the credential reads `Unreadable`.
#[test]
fn a_refused_delete_of_an_unparsable_item_records_nothing() {
    for (code, error) in refusals() {
        let rig = rig();
        sign_in(&rig);
        rig.store.put("case-law", b"{not json");
        let state = state_file(&rig);
        let (_connectors, handle) = rig.restart();
        rig.store.fail_deletes_with(Some(error));
        assert_eq!(handle.status(), CredentialStatus::Unreadable, "{code}");
        assert_eq!(rig.store.peek("case-law").unwrap(), b"{not json", "{code}");
        assert_eq!(state_file(&rig), state, "{code}");
    }
}

/// A sign-out whose delete is refused: the token was revoked, but nothing records a sign-out
/// that did not happen; the item stays, `Refused`, and the next look reads it again.
#[test]
fn a_refused_delete_at_sign_out_keeps_the_item_and_the_state() {
    for (code, error) in refusals() {
        let rig = rig();
        sign_in(&rig);
        rig.handle.status();
        let stored = rig.store.peek("case-law").unwrap();
        let state = state_file(&rig);
        rig.store.fail_deletes_with(Some(error.clone()));
        rig.store.fail_reads_with(Some(error));
        let out = rig.handle.sign_out().unwrap_err();
        assert!(
            matches!(&out, Error::Store(e) if refused(e)),
            "{code}: {out:?}"
        );
        assert_eq!(rig.store.peek("case-law").unwrap(), stored, "{code}");
        assert_eq!(state_file(&rig), state, "{code}");
        assert_eq!(rig.handle.status(), CredentialStatus::Unreadable, "{code}");
    }
}

/// An API key's 401 deletes it, unless the keychain refuses the delete: then the item and the
/// state file stay.
#[test]
fn a_refused_delete_after_a_revoked_key_records_nothing() {
    for (code, error) in refusals() {
        let rig = manual_rig();
        let key = rig.fake.new_api_key();
        rig.handle.add_api_key(&key).unwrap();
        let stored = rig.store.peek("case-law").unwrap();
        let state = state_file(&rig);
        rig.fake.revoke_all();
        rig.store.fail_deletes_with(Some(error));
        rig.clock().advance(Duration::from_secs(120));
        let out = rig.handle.account();
        assert!(
            matches!(&out, Err(Error::Auth(AuthError::Store(e))) if refused(e)),
            "{code}: {out:?}"
        );
        assert_eq!(rig.store.peek("case-law").unwrap(), stored, "{code}");
        assert_eq!(state_file(&rig), state, "{code}");
    }
}

/// A write refused at a sign-in or a new key: the old item and the state file stay, the new
/// tokens are revoked, and the credential reads `Unreadable` while reads are refused too.
#[test]
fn a_refused_write_keeps_the_old_item_and_the_state() {
    for (code, error) in refusals() {
        let rig = rig();
        sign_in(&rig);
        let stored = rig.store.peek("case-law").unwrap();
        let state = state_file(&rig);
        let pending = rig.handle.begin_sign_in(None).unwrap();
        rig.store.fail_writes_with(Some(error.clone()));
        rig.fake.clear_requests();
        let (_page, result) = complete(&rig.fake, pending);
        let err = result.unwrap_err();
        assert!(
            matches!(&err, SignInError::Store(e) if refused(e)),
            "{code}: {err:?}"
        );
        assert_eq!(
            rig.fake.requests_to("/revoke").len(),
            1,
            "{code}: the new token"
        );
        let key = rig.fake.new_api_key();
        let out = rig.handle.add_api_key(&key).unwrap_err();
        assert!(
            matches!(&out, Error::Store(e) if refused(e)),
            "{code}: {out:?}"
        );
        assert_eq!(rig.store.peek("case-law").unwrap(), stored, "{code}");
        assert_eq!(state_file(&rig), state, "{code}");
        rig.store.fail_reads_with(Some(error));
        assert_eq!(rig.handle.status(), CredentialStatus::Unreadable, "{code}");
    }
}

/// The refresh's own write refused twice (the rotated tokens): the old item and the state file
/// stay; the new tokens are held in memory (`persist_failed`, the app's "could not be saved").
#[test]
fn a_refused_write_after_rotation_keeps_the_old_item_and_the_state() {
    for (code, error) in refusals() {
        let rig = manual_rig();
        sign_in(&rig);
        let stored = rig.store.peek("case-law").unwrap();
        let state = state_file(&rig);
        rig.clock().advance(Duration::from_secs(3600));
        rig.store.fail_writes_with(Some(error));
        rig.handle.bearer().unwrap();
        assert!(rig.handle.persist_failed(), "{code}");
        assert_eq!(rig.store.peek("case-law").unwrap(), stored, "{code}");
        assert_eq!(state_file(&rig), state, "{code}");
        assert_eq!(rig.store.deletes(), 0, "{code}");
    }
}

/// A signed-in rig whose last read of the item was refused: a new key's check worked, but the
/// keychain refused the read of the item it would replace (`read_refused`, the key not stored).
fn last_read_refused(error: StoreError) -> (Rig, Vec<u8>, String) {
    let rig = manual_rig();
    let key = rig.fake.new_api_key();
    rig.handle.add_api_key(&key).unwrap();
    rig.handle.status();
    let stored = rig.store.peek("case-law").unwrap();
    let state = state_file(&rig);
    rig.store.fail_reads_with(Some(error));
    let other = rig.fake.new_api_key();
    let out = rig.handle.add_api_key(&other).unwrap_err();
    assert!(matches!(&out, Error::Store(e) if refused(e)), "{out:?}");
    (rig, stored, state)
}

/// `end_credential`'s automatic delete (a key the connector refused) does not run while the
/// last read of the item was refused: the item and the state file stay, and the credential reads
/// `Unreadable`. Once a read works, it runs.
#[test]
fn an_automatic_delete_waits_while_the_last_read_was_refused() {
    for (code, error) in refusals() {
        let (rig, stored, state) = last_read_refused(error);
        rig.fake.revoke_all();
        rig.clock().advance(Duration::from_secs(120));
        let out = rig.handle.account();
        assert!(
            matches!(&out, Err(Error::Auth(AuthError::Store(e))) if refused(e)),
            "{code}: {out:?}"
        );
        assert_eq!(rig.store.deletes(), 0, "{code}");
        assert_eq!(rig.store.peek("case-law").unwrap(), stored, "{code}");
        assert_eq!(state_file(&rig), state, "{code}");
        assert_eq!(rig.handle.status(), CredentialStatus::Unreadable, "{code}");
        rig.store.fail_reads_with(None);
        assert_eq!(
            rig.handle.account(),
            Err(Error::Auth(AuthError::KeyRevoked)),
            "{code}"
        );
        assert!(rig.store.peek("case-law").is_none(), "{code}");
    }
}

/// Sign out after a later read of the item was refused reads it again first: still refused, the
/// sign-out is refused and nothing is deleted; readable, it goes ahead.
#[test]
fn sign_out_reads_again_after_a_later_refused_read() {
    for (code, error) in refusals() {
        let (rig, stored, state) = last_read_refused(error);
        let out = rig.handle.sign_out().unwrap_err();
        assert!(
            matches!(&out, Error::Store(e) if refused(e)),
            "{code}: {out:?}"
        );
        assert_eq!(rig.store.deletes(), 0, "{code}");
        assert_eq!(rig.store.peek("case-law").unwrap(), stored, "{code}");
        assert_eq!(state_file(&rig), state, "{code}");
        rig.store.fail_reads_with(None);
        rig.handle.sign_out().unwrap();
        assert!(rig.store.peek("case-law").is_none(), "{code}");
        assert_eq!(rig.handle.status(), CredentialStatus::SignedOut, "{code}");
    }
}
