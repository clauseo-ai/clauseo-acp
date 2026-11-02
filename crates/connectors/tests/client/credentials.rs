//! The API key, sign-out and removal.

use clauseo_connectors::{AuthError, CredentialKind, CredentialStatus, Error, SignOutReport};

use crate::support::*;

#[test]
fn a_malformed_key_is_refused_before_any_request() {
    let rig = rig();
    for bad in [
        "",
        "cdk_live_",
        "cdk_live_0123456789abcdefghijABCDEFGHIJ0",
        "cdk_live_0123456789abcdefghijABCDEFGHIJ012",
        "cdk_test_0123456789abcdefghijABCDEFGHIJ01",
        "cdk_live_0123456789abcdefghij_BCDEFGHIJ01",
        " cdk_live_0123456789abcdefghijABCDEFGHIJ01",
    ] {
        assert_eq!(
            rig.handle.add_api_key(bad),
            Err(Error::BadKeyFormat),
            "{bad:?}"
        );
    }
    assert_eq!(rig.fake.requests().len(), 0);
}

#[test]
fn an_accepted_key_is_stored_with_kind_api_key() {
    let rig = rig();
    let key = rig.fake.new_api_key();
    rig.handle.add_api_key(&key).unwrap();
    let account = rig.fake.requests_to("/v1/case-law/account");
    assert_eq!(account.len(), 1);
    assert_eq!(
        account[0].header("authorization"),
        Some(format!("Bearer {key}").as_str())
    );
    let item = item(&rig).unwrap();
    assert_eq!(
        item,
        serde_json::json!({"v": 1, "kind": "api_key", "key": key})
    );
    assert!(state_file(&rig).contains(r#""case-law":{"kind":"api_key","ended":null}"#));
    assert_eq!(
        rig.handle.status(),
        CredentialStatus::SignedIn(CredentialKind::ApiKey)
    );
    assert_eq!(rig.handle.bearer().unwrap().0.expose(), key);
}

#[test]
fn a_refused_key_is_not_stored() {
    let rig = rig();
    let err = rig
        .handle
        .add_api_key("cdk_live_0123456789abcdefghijABCDEFGHIJ01")
        .unwrap_err();
    assert_eq!(err, Error::Auth(AuthError::KeyRevoked));
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(state_file(&rig), "");
    assert_eq!(rig.handle.status(), CredentialStatus::NotAdded);
}

#[test]
fn a_later_401_is_key_revoked_with_no_refresh() {
    let rig = manual_rig();
    let key = rig.fake.new_api_key();
    rig.handle.add_api_key(&key).unwrap();
    rig.fake.revoke_all();
    rig.clock().advance(std::time::Duration::from_secs(120));
    assert_eq!(
        rig.handle.account(),
        Err(Error::Auth(AuthError::KeyRevoked))
    );
    assert_eq!(rig.fake.requests_to("/token").len(), 0);
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(rig.handle.status(), CredentialStatus::KeyRevoked);
    assert!(state_file(&rig).contains(r#""ended":"key_revoked""#));
    assert_eq!(rig.handle.bearer(), Err(AuthError::KeyRevoked));
}

#[test]
fn sign_out_revokes_the_refresh_token_then_deletes_the_item() {
    let rig = rig();
    sign_in(&rig);
    let refresh = item(&rig).unwrap()["refresh_token"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(rig.handle.sign_out(), Ok(SignOutReport { revoked: true }));
    let revoke = rig.fake.requests_to("/revoke");
    assert_eq!(revoke.len(), 1);
    let form = revoke[0].form();
    let client_id = format!("{}/oauth/clients/clauseo-desktop.json", rig.fake.base());
    assert_eq!(
        form,
        [
            ("token".to_owned(), refresh),
            ("token_type_hint".to_owned(), "refresh_token".to_owned()),
            ("client_id".to_owned(), client_id),
        ]
    );
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(rig.handle.status(), CredentialStatus::SignedOut);
}

#[test]
fn a_revoke_answering_500_still_deletes_the_item() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.fail_next("/revoke", 500);
    assert_eq!(rig.handle.sign_out(), Ok(SignOutReport { revoked: false }));
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(rig.handle.status(), CredentialStatus::SignedOut);
}

#[test]
fn an_api_key_is_deleted_with_no_revoke_call() {
    let rig = rig();
    let key = rig.fake.new_api_key();
    rig.handle.add_api_key(&key).unwrap();
    assert_eq!(rig.handle.sign_out(), Ok(SignOutReport { revoked: false }));
    assert_eq!(rig.fake.requests_to("/revoke").len(), 0);
    assert!(rig.store.peek("case-law").is_none());
}

#[test]
fn bearer_after_sign_out_reads_nothing_and_sends_nothing() {
    let rig = rig();
    sign_in(&rig);
    rig.handle.sign_out().unwrap();
    rig.fake.clear_requests();
    let reads = rig.store.reads();
    assert_eq!(rig.handle.bearer(), Err(AuthError::SignedOut));
    assert_eq!(rig.store.reads(), reads);
    assert_eq!(rig.fake.requests().len(), 0);
}

#[test]
fn remove_signs_out_then_forgets_the_connector() {
    let rig = rig();
    sign_in(&rig);
    rig.handle.remove().unwrap();
    assert_eq!(rig.fake.requests_to("/revoke").len(), 1);
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(state_file(&rig).trim(), r#"{"v":1,"connectors":{}}"#);
    assert_eq!(rig.handle.status(), CredentialStatus::NotAdded);
    assert_eq!(rig.handle.bearer(), Err(AuthError::NotAdded));
}

#[test]
fn signing_in_again_after_sign_out_works() {
    let rig = rig();
    sign_in(&rig);
    rig.handle.sign_out().unwrap();
    sign_in(&rig);
    assert_eq!(
        rig.handle.status(),
        CredentialStatus::SignedIn(CredentialKind::OAuth)
    );
    assert!(state_file(&rig).contains(r#""ended":null"#));
}
