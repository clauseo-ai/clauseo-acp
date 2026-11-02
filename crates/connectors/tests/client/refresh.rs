//! `bearer` and the refresh: before expiry, single-flight, rotation, `invalid_grant`, the 401
//! retry, and a keychain that refuses the rotated token.

use std::collections::HashSet;
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::Duration;

use clauseo_connectors::{AuthError, CredentialStatus, Error, McpRequestHeaders};
use serde_json::json;

use crate::support::*;

fn tools_call() -> Vec<u8> {
    json!({"jsonrpc": "2.0", "id": 3, "method": "tools/call",
        "params": {"name": "citator", "arguments": {}}})
    .to_string()
    .into_bytes()
}

#[test]
fn a_token_with_more_than_5_minutes_left_is_used_without_a_request() {
    let rig = manual_rig();
    sign_in(&rig);
    rig.fake.clear_requests();
    rig.clock().advance(Duration::from_secs(3600 - 301));
    let first = rig.handle.bearer().unwrap();
    assert_eq!(rig.fake.requests().len(), 0);
    assert_eq!(first.0.expose(), item(&rig).unwrap()["access_token"]);
}

#[test]
fn a_token_within_5_minutes_of_expiry_is_refreshed_first() {
    let rig = manual_rig();
    sign_in(&rig);
    let before = rig.handle.bearer().unwrap();
    rig.clock().advance(Duration::from_secs(3600 - 299));
    let after = rig.handle.bearer().unwrap();
    assert_ne!(before, after);
    let refresh = rig.fake.requests_to("/token");
    assert_eq!(refresh.len(), 2, "the exchange and one refresh");
    let form = refresh[1].form();
    let names: Vec<&str> = form.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "grant_type",
            "refresh_token",
            "client_id",
            "resource",
            "resource"
        ]
    );
    assert_eq!(form[0].1, "refresh_token");
}

#[test]
fn eight_threads_on_an_expired_token_cause_one_refresh() {
    let rig = manual_rig();
    sign_in(&rig);
    rig.clock().advance(Duration::from_secs(7200));
    rig.fake.clear_requests();
    let barrier = Arc::new(Barrier::new(8));
    let mut threads = Vec::new();
    for _ in 0..8 {
        let handle = rig.handle.clone();
        let barrier = Arc::clone(&barrier);
        threads.push(thread::spawn(move || {
            barrier.wait();
            handle.bearer().unwrap()
        }));
    }
    let tokens: Vec<_> = threads.into_iter().map(|t| t.join().unwrap()).collect();
    assert_eq!(rig.fake.requests_to("/token").len(), 1);
    assert!(tokens.iter().all(|token| *token == tokens[0]));
}

#[test]
fn a_rotated_refresh_token_replaces_the_old_one_in_the_store() {
    let rig = manual_rig();
    sign_in(&rig);
    let old = item(&rig).unwrap()["refresh_token"].clone();
    rig.clock().advance(Duration::from_secs(3600));
    rig.handle.bearer().unwrap();
    let new = item(&rig).unwrap();
    assert_ne!(new["refresh_token"], old);
    assert_eq!(new["access_token"], rig.handle.bearer().unwrap().0.expose());
    assert!(!rig.handle.persist_failed());
}

#[test]
fn invalid_grant_signs_out_deletes_the_item_and_keeps_the_connector_added() {
    let rig = manual_rig();
    sign_in(&rig);
    rig.fake.revoke_all();
    rig.clock().advance(Duration::from_secs(3600));
    assert_eq!(rig.handle.bearer(), Err(AuthError::SignedOut));
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(rig.handle.status(), CredentialStatus::SignedOut);
    assert!(state_file(&rig).contains(r#""ended":"signed_out""#));
    // The status survives a restart.
    let (_connectors, handle) = rig.restart();
    assert_eq!(handle.status(), CredentialStatus::SignedOut);
}

#[test]
fn a_network_error_on_refresh_keeps_the_credential() {
    let rig = manual_rig();
    sign_in(&rig);
    rig.clock().advance(Duration::from_secs(3600));
    rig.fake.fail_next("/token", 503);
    assert!(matches!(rig.handle.bearer(), Err(AuthError::Network(_))));
    assert!(rig.store.peek("case-law").is_some());
    rig.handle.bearer().unwrap();
}

#[test]
fn a_401_refreshes_once_and_retries_and_a_second_401_signs_out() {
    let rig = rig();
    sign_in(&rig);
    rig.fake.expire_access_tokens();
    let reply = rig
        .handle
        .mcp_post(&tools_call(), &McpRequestHeaders::default())
        .unwrap();
    assert_eq!(reply.status, 200);
    let mcp = rig.fake.requests_to("/mcp/case-law");
    assert_eq!(mcp.len(), 2);
    assert_ne!(
        mcp[0].header("authorization"),
        mcp[1].header("authorization")
    );
    assert_eq!(rig.fake.requests_to("/token").len(), 2);

    rig.fake.clear_requests();
    rig.fake.fail_next("/mcp/case-law", 401);
    // The fake answers 401 once by the knob, then the retry with a fresh token passes.
    rig.handle
        .mcp_post(&tools_call(), &McpRequestHeaders::default())
        .unwrap();
    assert_eq!(rig.fake.requests_to("/token").len(), 1);

    // Every token refused: one refresh, one retry, then signed out.
    rig.fake.clear_requests();
    rig.fake.expire_access_tokens();
    rig.fake.set_access_ttl(Duration::ZERO);
    let err = rig
        .handle
        .mcp_post(&tools_call(), &McpRequestHeaders::default())
        .unwrap_err();
    assert_eq!(err, Error::Auth(AuthError::SignedOut));
    assert_eq!(rig.fake.requests_to("/mcp/case-law").len(), 2);
    assert_eq!(rig.fake.requests_to("/token").len(), 1);
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(rig.handle.status(), CredentialStatus::SignedOut);
}

#[test]
fn a_failed_keychain_write_after_rotation_keeps_the_tokens_in_memory() {
    let rig = manual_rig();
    sign_in(&rig);
    let stored = item(&rig).unwrap();
    rig.clock().advance(Duration::from_secs(3600));
    rig.store.fail_next_writes(2);
    let bearer = rig.handle.bearer().unwrap();
    assert!(rig.handle.persist_failed());
    assert_eq!(
        item(&rig).unwrap(),
        stored,
        "the keychain still holds the old tokens"
    );
    assert_ne!(bearer.0.expose(), stored["access_token"]);
    assert_eq!(rig.handle.bearer().unwrap(), bearer, "bearer keeps working");
}

#[test]
fn a_keychain_write_that_fails_once_is_retried() {
    let rig = manual_rig();
    sign_in(&rig);
    rig.clock().advance(Duration::from_secs(3600));
    rig.store.fail_next_writes(1);
    let bearer = rig.handle.bearer().unwrap();
    assert!(!rig.handle.persist_failed());
    assert_eq!(item(&rig).unwrap()["access_token"], bearer.0.expose());
}

#[test]
fn bearer_reads_the_keychain_once() {
    let rig = rig();
    sign_in(&rig);
    let (_connectors, handle) = rig.restart();
    let reads = rig.store.reads();
    handle.status();
    handle.bearer().unwrap();
    handle.status();
    handle.bearer().unwrap();
    assert_eq!(rig.store.reads(), reads + 1);
}

#[test]
fn an_item_that_does_not_parse_is_deleted_and_signed_out() {
    let rig = rig();
    sign_in(&rig);
    rig.store.put("case-law", b"{not json");
    let (_connectors, handle) = rig.restart();
    assert_eq!(handle.status(), CredentialStatus::SignedOut);
    assert!(rig.store.peek("case-law").is_none());
    assert_eq!(handle.bearer(), Err(AuthError::SignedOut));
}

#[test]
fn bearer_without_a_credential_is_not_added() {
    let rig = rig();
    assert_eq!(rig.handle.bearer(), Err(AuthError::NotAdded));
    assert_eq!(rig.store.reads(), 0);
}

/// The refresh tokens presented at `/token` for a refresh, in order.
fn presented_refresh_tokens(rig: &Rig) -> Vec<String> {
    rig.fake
        .requests_to("/token")
        .iter()
        .filter_map(|request| {
            let form = request.form();
            let refresh = form
                .iter()
                .any(|(k, v)| k == "grant_type" && v == "refresh_token");
            let token = form.iter().find(|(k, _)| k == "refresh_token");
            refresh.then(|| token.map(|(_, v)| v.clone())).flatten()
        })
        .collect()
}

/// Two `Connectors` on one store and one state file (two processes, say): A refreshes and
/// rotates; B, holding A's spent refresh token in memory, adopts A's tokens from the keychain
/// instead of refreshing with the spent one, and no refresh token is ever presented twice.
#[test]
fn a_second_instance_adopts_the_rotated_chain_instead_of_forking_it() {
    let rig = manual_rig();
    sign_in(&rig);
    let (_b_connectors, b) = rig.restart();
    let first = b.bearer().unwrap();
    assert_eq!(first, rig.handle.bearer().unwrap());
    let spent = item(&rig).unwrap()["refresh_token"]
        .as_str()
        .unwrap()
        .to_owned();

    // Within 5 minutes of expiry: A refreshes and rotates.
    rig.clock().advance(Duration::from_secs(3600 - 200));
    let a_token = rig.handle.bearer().unwrap();
    assert_eq!(presented_refresh_tokens(&rig), [spent.as_str()]);
    // B adopts A's fresh access token: no refresh at all.
    assert_eq!(b.bearer().unwrap(), a_token);
    assert_eq!(presented_refresh_tokens(&rig), [spent.as_str()]);

    // Well past the 30 s grace, A's token is in its window again. B refreshes first, with
    // the stored (current) refresh token; then A adopts B's tokens.
    rig.clock().advance(Duration::from_secs(3600 - 200));
    let b_token = b.bearer().unwrap();
    assert_eq!(rig.handle.bearer().unwrap(), b_token);
    let presented = presented_refresh_tokens(&rig);
    assert_eq!(presented.len(), 2, "{presented:?}");
    assert_ne!(presented[1], spent, "B refreshed with A's spent token");
    // The chain is intact: one more refresh still works.
    rig.clock().advance(Duration::from_secs(3600));
    rig.handle.bearer().unwrap();
    b.bearer().unwrap();
    let presented = presented_refresh_tokens(&rig);
    assert_eq!(presented.len(), 3, "{presented:?}");
    let unique: HashSet<&String> = presented.iter().collect();
    assert_eq!(
        unique.len(),
        3,
        "a refresh token was presented twice: {presented:?}"
    );
    assert_eq!(
        rig.handle.status(),
        CredentialStatus::SignedIn(clauseo_connectors::CredentialKind::OAuth)
    );
}

/// The other instance signed out (the item is gone): the refresh is not tried, signed out.
#[test]
fn an_item_deleted_by_another_instance_is_signed_out_at_the_next_refresh() {
    let rig = manual_rig();
    sign_in(&rig);
    rig.handle.bearer().unwrap();
    let (_b_connectors, b) = rig.restart();
    b.sign_out().unwrap();
    rig.fake.clear_requests();
    rig.clock().advance(Duration::from_secs(3600));
    assert_eq!(rig.handle.bearer(), Err(AuthError::SignedOut));
    assert_eq!(rig.fake.requests_to("/token").len(), 0);
}

/// A deviation from the brief's literal rule: a refresh that fails on the network while the
/// access token has not expired yet returns that token (with a warning); once it has
/// expired, the failure is `Network`.
#[test]
fn a_network_failure_before_expiry_returns_the_current_token() {
    let rig = manual_rig();
    sign_in(&rig);
    let current = rig.handle.bearer().unwrap();
    rig.clock().advance(Duration::from_secs(3600 - 200));
    rig.fake.close_next("/token");
    assert_eq!(rig.handle.bearer().unwrap(), current);
    assert_eq!(
        presented_refresh_tokens(&rig).len(),
        1,
        "the refresh was tried"
    );
    rig.clock().advance(Duration::from_secs(300));
    rig.fake.close_next("/token");
    assert!(matches!(rig.handle.bearer(), Err(AuthError::Network(_))));
    assert!(rig.store.peek("case-law").is_some());
    // The connector is back: the refresh works.
    assert_ne!(rig.handle.bearer().unwrap(), current);
}

/// One thread's request with the old token is answered 401 after another thread already
/// refreshed: it takes the newer token and retries, with no second refresh.
#[test]
fn a_401_after_another_thread_refreshed_takes_the_newer_token() {
    let rig = rig();
    sign_in(&rig);
    let old = rig.handle.bearer().unwrap();
    rig.fake.clear_requests();
    // A's request is held 600 ms and then refused; B's is refused at once.
    rig.fake
        .delay_next("/mcp/case-law", Duration::from_millis(600));
    rig.fake.fail_next("/mcp/case-law", 401);
    rig.fake.fail_next("/mcp/case-law", 401);
    let a = {
        let handle = rig.handle.clone();
        thread::spawn(move || handle.mcp_post(&tools_call(), &McpRequestHeaders::default()))
    };
    while rig.fake.requests_to("/mcp/case-law").is_empty() {
        thread::park_timeout(Duration::from_millis(5));
    }
    // Long enough for A's connection to take the queued delay before B's arrives.
    clauseo_connectors::CancelFlag::new().wait_timeout(Duration::from_millis(50));
    // B: 401, the one refresh, and a retry with the new token.
    rig.handle
        .mcp_post(&tools_call(), &McpRequestHeaders::default())
        .unwrap();
    let new = rig.handle.bearer().unwrap();
    assert_ne!(new, old);
    assert_eq!(a.join().unwrap().unwrap().status, 200);
    assert_eq!(rig.fake.requests_to("/token").len(), 1, "one refresh");
    let mcp = rig.fake.requests_to("/mcp/case-law");
    assert_eq!(mcp.len(), 4);
    let new_header = format!("Bearer {}", new.0.expose());
    assert_eq!(
        mcp[3].header("authorization"),
        Some(new_header.as_str()),
        "A retried with the newer token"
    );
}

/// A 5xx from the token endpoint is treated like a network failure: before expiry the current
/// token comes back; once it has expired, `Network`.
#[test]
fn a_5xx_on_refresh_before_expiry_returns_the_current_token() {
    let rig = manual_rig();
    sign_in(&rig);
    let current = rig.handle.bearer().unwrap();
    rig.clock().advance(Duration::from_secs(3600 - 200));
    rig.fake.fail_next("/token", 502);
    assert_eq!(rig.handle.bearer().unwrap(), current);
    assert_eq!(
        presented_refresh_tokens(&rig).len(),
        1,
        "the refresh was tried"
    );
    rig.clock().advance(Duration::from_secs(300));
    rig.fake.fail_next("/token", 500);
    assert!(matches!(rig.handle.bearer(), Err(AuthError::Network(_))));
    assert!(rig.store.peek("case-law").is_some());
    assert_ne!(rig.handle.bearer().unwrap(), current);
}
