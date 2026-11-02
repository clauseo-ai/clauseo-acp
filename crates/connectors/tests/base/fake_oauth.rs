//! The fake's authorization server refuses what the real one would.

use std::sync::Arc;
use std::time::Duration;

use clauseo_connectors::ManualClock;
use clauseo_connectors::fake::{FakeConnector, FakeHandle, FakeOptions, http};
use serde_json::Value;

use crate::support::*;

fn fake_with(clock: &Arc<ManualClock>) -> FakeHandle {
    FakeConnector::start(FakeOptions {
        clock: Arc::clone(clock) as _,
        ..FakeOptions::default()
    })
    .unwrap()
}

fn error(body: &Value) -> &str {
    body["error"].as_str().unwrap_or_default()
}

#[test]
fn authorize_redirects_with_code_state_and_iss() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let pairs = authorize(&fake);
    assert!(param(&pairs, "code").unwrap().starts_with("fake_code_"));
    assert_eq!(param(&pairs, "state"), Some("state-123"));
    assert_eq!(param(&pairs, "iss"), Some(fake.base()));

    fake.omit_iss();
    assert_eq!(param(&authorize(&fake), "iss"), None);
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    fake.wrong_iss("http://127.0.0.1:1");
    assert_eq!(param(&authorize(&fake), "iss"), Some("http://127.0.0.1:1"));
    fake.deny_authorize(Some("access_denied"));
    let pairs = authorize(&fake);
    assert_eq!(param(&pairs, "error"), Some("access_denied"));
    assert_eq!(param(&pairs, "code"), None);
}

#[test]
fn authorize_refuses_each_wrong_parameter() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let cases: [(&str, Option<&str>); 13] = [
        (
            "client_id",
            Some("https://data.clauseo.chat/oauth/clients/clauseo-desktop.json"),
        ),
        ("redirect_uri", Some("http://localhost:5555/oauth/callback")),
        ("redirect_uri", Some("http://127.0.0.1/oauth/callback")),
        ("redirect_uri", Some("http://127.0.0.1:5555/callback")),
        (
            "redirect_uri",
            Some("https://127.0.0.1:5555/oauth/callback"),
        ),
        ("redirect_uri", Some("clauseo://oauth/callback")),
        ("response_type", Some("token")),
        ("code_challenge_method", Some("plain")),
        ("code_challenge_method", None),
        ("code_challenge", Some("short")),
        ("scope", Some("case-law")),
        ("state", None),
        ("resource", None),
    ];
    for (name, value) in cases {
        let mut params: Vec<(String, String)> = authorize_params(&fake)
            .into_iter()
            .filter(|(key, _)| key != name)
            .collect();
        if let Some(value) = value {
            params.push((name.to_owned(), value.to_owned()));
        }
        let response = authorize_with(&fake, &params);
        assert_eq!(response.status, 400, "{name}={value:?}");
        assert_eq!(response.text(), name);
    }
    // One resource of the two.
    let mut params: Vec<(String, String)> = authorize_params(&fake);
    params.pop();
    assert_eq!(authorize_with(&fake, &params).status, 400);
}

#[test]
fn the_exchange_checks_pkce_redirect_and_client() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let code = code(&fake);
    let other = "x".repeat(43);
    let (status, body) = exchange(&fake, &code, &other, REDIRECT);
    assert_eq!(
        (status, error(&body)),
        (400, "invalid_grant"),
        "PKCE mismatch"
    );
    // The failed attempt spent the code.
    let (status, body) = exchange(&fake, &code, VERIFIER, REDIRECT);
    assert_eq!(
        (status, error(&body)),
        (400, "invalid_grant"),
        "a spent code"
    );

    let code = crate::support::code(&fake);
    let (status, body) = exchange(
        &fake,
        &code,
        VERIFIER,
        "http://127.0.0.1:5556/oauth/callback",
    );
    assert_eq!(
        (status, error(&body)),
        (400, "invalid_grant"),
        "another redirect_uri"
    );

    let code = crate::support::code(&fake);
    let (status, body) = token(
        &fake,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", REDIRECT),
            ("client_id", "https://evil.example/client.json"),
            ("code_verifier", VERIFIER),
        ],
    );
    assert_eq!((status, error(&body)), (401, "invalid_client"));

    let code = crate::support::code(&fake);
    let client_id = client_id(&fake);
    let (status, body) = token(
        &fake,
        &[
            ("grant_type", "authorization_code"),
            ("code", &code),
            ("redirect_uri", REDIRECT),
            ("client_id", &client_id),
            ("code_verifier", VERIFIER),
        ],
    );
    assert_eq!(
        (status, error(&body)),
        (400, "invalid_target"),
        "no resources"
    );
}

#[test]
fn a_good_exchange_answers_tokens_and_a_reused_code_revokes_them() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let code = code(&fake);
    let (status, body) = exchange(&fake, &code, VERIFIER, REDIRECT);
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["token_type"], "Bearer");
    assert_eq!(body["expires_in"], 3600);
    let access = body["access_token"].as_str().unwrap();
    assert!(access.len() < 64);
    assert_eq!(get(&fake, "/v1/case-law/account", Some(access)).status, 200);

    let (status, body) = exchange(&fake, &code, VERIFIER, REDIRECT);
    assert_eq!((status, error(&body)), (400, "invalid_grant"));
    assert_eq!(get(&fake, "/v1/case-law/account", Some(access)).status, 401);
    let secrets = fake.issued_secrets();
    assert!(secrets.contains(&code) && secrets.iter().any(|secret| secret == access));
}

#[test]
fn a_code_expires_after_60_seconds_or_on_the_knob() {
    let clock = Arc::new(ManualClock::default());
    let fake = fake_with(&clock);
    let code = code(&fake);
    clock.advance(Duration::from_secs(61));
    let (status, body) = exchange(&fake, &code, VERIFIER, REDIRECT);
    assert_eq!((status, error(&body)), (400, "invalid_grant"));

    let code = crate::support::code(&fake);
    clock.advance(Duration::from_secs(59));
    assert_eq!(exchange(&fake, &code, VERIFIER, REDIRECT).0, 200);

    let code = crate::support::code(&fake);
    fake.expire_codes();
    let (status, body) = exchange(&fake, &code, VERIFIER, REDIRECT);
    assert_eq!((status, error(&body)), (400, "invalid_grant"));
}

#[test]
fn rotation_keeps_the_old_token_for_30_seconds_then_reuse_revokes_the_chain() {
    let clock = Arc::new(ManualClock::default());
    let fake = fake_with(&clock);
    let (_, rt1) = sign_in(&fake);

    let (status, body) = refresh(&fake, &rt1);
    assert_eq!(status, 200, "{body}");
    let rt2 = body["refresh_token"].as_str().unwrap().to_owned();
    assert_ne!(rt2, rt1);
    // rt2 unused: rt1 still works and gives rt2 again, so the chain never forks.
    let (status, body) = refresh(&fake, &rt1);
    assert_eq!(
        (status, body["refresh_token"].as_str()),
        (200, Some(rt2.as_str()))
    );

    // rt2's first use starts the grace.
    let (status, body) = refresh(&fake, &rt2);
    assert_eq!(status, 200);
    let rt3 = body["refresh_token"].as_str().unwrap().to_owned();
    let access3 = body["access_token"].as_str().unwrap().to_owned();
    clock.advance(Duration::from_secs(30));
    assert_eq!(refresh(&fake, &rt1).0, 200, "within the grace");

    clock.advance(Duration::from_secs(1));
    let (status, body) = refresh(&fake, &rt1);
    assert_eq!(
        (status, error(&body)),
        (400, "invalid_grant"),
        "after the grace"
    );
    let (status, body) = refresh(&fake, &rt3);
    assert_eq!(
        (status, error(&body)),
        (400, "invalid_grant"),
        "the chain is revoked"
    );
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access3)).status,
        401
    );
}

#[test]
fn revoke_ends_the_chain_and_answers_200_for_anything() {
    let fake = FakeConnector::start(FakeOptions::default()).unwrap();
    let (access, rt) = sign_in(&fake);
    let client_id = client_id(&fake);
    for token in [rt.as_str(), "not-a-token"] {
        let body = url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs([
                ("token", token),
                ("token_type_hint", "refresh_token"),
                ("client_id", &client_id),
            ])
            .finish();
        let response = http(
            "POST",
            &fake.url("/revoke"),
            &[("Content-Type", "application/x-www-form-urlencoded")],
            body.as_bytes(),
        )
        .unwrap();
        assert_eq!(response.status, 200);
    }
    assert_eq!(refresh(&fake, &rt).0, 400);
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access)).status,
        401
    );
}

#[test]
fn the_bearer_check_follows_expiry_keys_and_revoke_all() {
    let clock = Arc::new(ManualClock::default());
    let fake = fake_with(&clock);
    let unauthorized = get(&fake, "/v1/case-law/account", None);
    assert_eq!(unauthorized.status, 401);
    assert!(
        unauthorized
            .header("www-authenticate")
            .unwrap()
            .contains("/.well-known/oauth-protected-resource/mcp/case-law")
    );

    fake.set_access_ttl(Duration::from_secs(10));
    let (access, rt) = sign_in(&fake);
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access)).status,
        200
    );
    clock.advance(Duration::from_secs(10));
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access)).status,
        401,
        "expired"
    );
    let (_, body) = refresh(&fake, &rt);
    let access = body["access_token"].as_str().unwrap().to_owned();
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access)).status,
        200
    );
    fake.expire_access_tokens();
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access)).status,
        401
    );

    let key = fake.new_api_key();
    assert!(key.starts_with("cdk_live_") && key.len() == 41);
    assert_eq!(get(&fake, "/v1/case-law/account", Some(&key)).status, 200);
    let (_, body) = refresh(&fake, &rt);
    let access = body["access_token"].as_str().unwrap().to_owned();
    fake.revoke_all();
    assert_eq!(get(&fake, "/v1/case-law/account", Some(&key)).status, 401);
    assert_eq!(
        get(&fake, "/v1/case-law/account", Some(&access)).status,
        401
    );
    assert_eq!(refresh(&fake, &rt).0, 400);
}
