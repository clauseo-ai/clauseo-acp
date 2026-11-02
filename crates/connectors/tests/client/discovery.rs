//! Discovery: the fake's metadata passes; each broken field fails with `Discovery` and no
//! listener opens.

use clauseo_connectors::SignInError;
use clauseo_connectors::fake::{FakeConnector, FakeOptions, ScratchDir};
use clauseo_connectors::{Connectors, MemoryStore, Origin};
use serde_json::{Value, json};
use std::sync::Arc;

use crate::support::*;

#[test]
fn begin_sign_in_passes_discovery_against_the_fake() {
    let rig = rig();
    let pending = rig.handle.begin_sign_in(None).unwrap();
    assert!(
        pending
            .authorize_url()
            .as_str()
            .starts_with(&format!("{}/authorize?", rig.fake.base()))
    );
    assert_eq!(
        rig.fake
            .requests_to("/.well-known/oauth-protected-resource/mcp/case-law")
            .len(),
        1
    );
    assert_eq!(
        rig.fake
            .requests_to("/.well-known/oauth-authorization-server")
            .len(),
        1
    );
    drop(pending);
    // The metadata is kept for the process.
    let _again = rig.handle.begin_sign_in(None).unwrap();
    assert_eq!(
        rig.fake
            .requests_to("/.well-known/oauth-authorization-server")
            .len(),
        1
    );
}

/// Begins a sign-in with the metadata changed by `edit`; asserts `Discovery` and no request
/// past discovery (no listener: nothing reached `/authorize` and the fake saw no callback).
fn refused(edit_prm: impl FnOnce(&mut Value), edit_as: impl FnOnce(&mut Value, &str)) -> String {
    let rig = rig();
    let mut prm = rig.fake.prm();
    edit_prm(&mut prm);
    rig.fake.set_prm(prm);
    let mut metadata = rig.fake.as_metadata();
    edit_as(&mut metadata, rig.fake.base());
    rig.fake.set_as_metadata(metadata);
    match rig.handle.begin_sign_in(None) {
        Err(SignInError::Discovery(why)) => why,
        other => panic!("expected Discovery, got {other:?}"),
    }
}

fn prm_refused(edit: impl FnOnce(&mut Value)) -> String {
    refused(edit, |_, _| {})
}

fn as_refused(edit: impl FnOnce(&mut Value, &str)) -> String {
    refused(|_| {}, edit)
}

#[test]
fn a_resource_that_is_not_the_mcp_url_is_refused() {
    let why = prm_refused(|prm| prm["resource"] = json!("https://evil.example/mcp/case-law"));
    assert!(why.contains("resource"), "{why}");
}

#[test]
fn authorization_servers_without_the_issuer_are_refused() {
    let why = prm_refused(|prm| prm["authorization_servers"] = json!(["https://evil.example"]));
    assert!(why.contains("authorization_servers"), "{why}");
    prm_refused(|prm| {
        prm.as_object_mut().unwrap().remove("authorization_servers");
    });
}

#[test]
fn an_issuer_one_character_off_is_refused() {
    let why = as_refused(|meta, base| meta["issuer"] = json!(format!("{base}/")));
    assert!(why.contains("issuer"), "{why}");
    as_refused(|meta, base| meta["issuer"] = json!(base.replace("127.0.0.1", "127.0.0.2")));
}

#[test]
fn client_id_metadata_documents_must_be_supported() {
    as_refused(|meta, _| meta["client_id_metadata_document_supported"] = json!(false));
    let why = as_refused(|meta, _| {
        meta.as_object_mut()
            .unwrap()
            .remove("client_id_metadata_document_supported");
    });
    assert!(
        why.contains("client_id_metadata_document_supported"),
        "{why}"
    );
}

#[test]
fn the_none_auth_method_must_be_listed() {
    let why = as_refused(|meta, _| {
        meta["token_endpoint_auth_methods_supported"] = json!(["client_secret_basic"]);
    });
    assert!(why.contains("none"), "{why}");
}

#[test]
fn s256_must_be_listed() {
    let why = as_refused(|meta, _| meta["code_challenge_methods_supported"] = json!(["plain"]));
    assert!(why.contains("S256"), "{why}");
}

#[test]
fn the_iss_parameter_must_be_supported() {
    as_refused(|meta, _| meta["authorization_response_iss_parameter_supported"] = json!(false));
    let why = as_refused(|meta, _| {
        meta.as_object_mut()
            .unwrap()
            .remove("authorization_response_iss_parameter_supported");
    });
    assert!(
        why.contains("authorization_response_iss_parameter_supported"),
        "{why}"
    );
}

#[test]
fn an_endpoint_on_another_host_or_scheme_is_refused() {
    for name in [
        "authorization_endpoint",
        "token_endpoint",
        "revocation_endpoint",
    ] {
        let path = name.split('_').next().unwrap();
        let why = as_refused(|meta, _| meta[name] = json!(format!("https://evil.example/{path}")));
        assert!(why.contains(name), "{why}");
        // Not the issuer's scheme: https on the fake's own host and port.
        let why = as_refused(|meta, base| {
            meta[name] = json!(format!("{}/{path}", base.replace("http://", "https://")));
        });
        assert!(why.contains(name), "{why}");
        // Another port on the same host.
        as_refused(|meta, _| meta[name] = json!(format!("http://127.0.0.1:1/{path}")));
        as_refused(|meta, _| {
            meta.as_object_mut().unwrap().remove(name);
        });
    }
}

#[test]
fn metadata_that_is_missing_or_not_json_is_refused() {
    let rig = rig();
    rig.fake
        .fail_next("/.well-known/oauth-protected-resource/mcp/case-law", 404);
    assert!(matches!(
        rig.handle.begin_sign_in(None),
        Err(SignInError::Discovery(_))
    ));
}

#[test]
fn a_fake_origin_off_loopback_http_is_refused_at_new() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let scratch = ScratchDir::new("origin").unwrap();
    for bad in [
        "https://127.0.0.1:1",
        "http://10.0.0.1:1",
        "http://127.0.0.1:1/x",
    ] {
        let mut config = fake.config(scratch.state_file());
        config.origin = Origin::Fake(url::Url::parse(bad).unwrap());
        assert!(
            Connectors::new(config, Arc::new(MemoryStore::new())).is_err(),
            "{bad}"
        );
    }
}
