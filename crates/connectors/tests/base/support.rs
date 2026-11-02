//! Driving the fake by hand: PKCE pairs, `/authorize`, `/token` and API calls over the fake's
//! plain test client.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use clauseo_connectors::fake::{FakeHandle, RawResponse, http};
use serde_json::Value;
use sha2::{Digest as _, Sha256};
use url::Url;

/// A 43-character verifier (RFC 7636 appendix B's).
pub(crate) const VERIFIER: &str = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
pub(crate) const REDIRECT: &str = "http://127.0.0.1:5555/oauth/callback";

pub(crate) fn challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub(crate) fn client_id(fake: &FakeHandle) -> String {
    format!("{}/oauth/clients/clauseo-desktop.json", fake.base())
}

/// The authorize parameters the desktop client sends.
pub(crate) fn authorize_params(fake: &FakeHandle) -> Vec<(String, String)> {
    let base = fake.base();
    [
        ("response_type", "code".to_owned()),
        ("client_id", client_id(fake)),
        ("redirect_uri", REDIRECT.to_owned()),
        ("scope", "case-law offline_access".to_owned()),
        ("state", "state-123".to_owned()),
        ("code_challenge", challenge(VERIFIER)),
        ("code_challenge_method", "S256".to_owned()),
        ("resource", format!("{base}/mcp/case-law")),
        ("resource", format!("{base}/v1/case-law")),
    ]
    .into_iter()
    .map(|(name, value)| (name.to_owned(), value))
    .collect()
}

pub(crate) fn authorize_with(fake: &FakeHandle, params: &[(String, String)]) -> RawResponse {
    let mut url = fake.url("/authorize");
    url.query_pairs_mut().extend_pairs(params);
    http("GET", &url, &[], &[]).unwrap()
}

/// `/authorize` with the client's parameters: the redirect's query pairs.
pub(crate) fn authorize(fake: &FakeHandle) -> Vec<(String, String)> {
    let response = authorize_with(fake, &authorize_params(fake));
    assert_eq!(response.status, 302, "{}", response.text());
    let location = Url::parse(response.header("location").unwrap()).unwrap();
    location.query_pairs().into_owned().collect()
}

pub(crate) fn param<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    pairs
        .iter()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

pub(crate) fn code(fake: &FakeHandle) -> String {
    param(&authorize(fake), "code").unwrap().to_owned()
}

/// `POST /token` with these form pairs; status and JSON body.
pub(crate) fn token(fake: &FakeHandle, form: &[(&str, &str)]) -> (u16, Value) {
    let body = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let response = http(
        "POST",
        &fake.url("/token"),
        &[("Content-Type", "application/x-www-form-urlencoded")],
        body.as_bytes(),
    )
    .unwrap();
    (
        response.status,
        serde_json::from_slice(&response.body).unwrap_or(Value::Null),
    )
}

/// The code exchange the desktop client sends.
pub(crate) fn exchange(
    fake: &FakeHandle,
    code: &str,
    verifier: &str,
    redirect: &str,
) -> (u16, Value) {
    let client_id = client_id(fake);
    let mcp = format!("{}/mcp/case-law", fake.base());
    let api = format!("{}/v1/case-law", fake.base());
    token(
        fake,
        &[
            ("grant_type", "authorization_code"),
            ("code", code),
            ("redirect_uri", redirect),
            ("client_id", &client_id),
            ("code_verifier", verifier),
            ("resource", &mcp),
            ("resource", &api),
        ],
    )
}

/// A signed-in chain: (access token, refresh token).
pub(crate) fn sign_in(fake: &FakeHandle) -> (String, String) {
    let code = code(fake);
    let (status, body) = exchange(fake, &code, VERIFIER, REDIRECT);
    assert_eq!(status, 200, "{body}");
    (
        body["access_token"].as_str().unwrap().to_owned(),
        body["refresh_token"].as_str().unwrap().to_owned(),
    )
}

pub(crate) fn refresh(fake: &FakeHandle, refresh_token: &str) -> (u16, Value) {
    let client_id = client_id(fake);
    token(
        fake,
        &[
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
            ("client_id", &client_id),
        ],
    )
}

/// `GET path` with an optional bearer token.
pub(crate) fn get(fake: &FakeHandle, path: &str, bearer: Option<&str>) -> RawResponse {
    let auth = bearer.map(|token| format!("Bearer {token}"));
    let headers: Vec<(&str, &str)> = auth
        .iter()
        .map(|auth| ("Authorization", auth.as_str()))
        .collect();
    http("GET", &fake.url(path), &headers, &[]).unwrap()
}

/// `POST path` with a JSON body and an optional bearer token.
pub(crate) fn post_json(
    fake: &FakeHandle,
    path: &str,
    bearer: Option<&str>,
    body: &Value,
) -> RawResponse {
    let auth = bearer.map(|token| format!("Bearer {token}"));
    let mut headers = vec![("Content-Type", "application/json")];
    headers.extend(auth.iter().map(|auth| ("Authorization", auth.as_str())));
    http(
        "POST",
        &fake.url(path),
        &headers,
        body.to_string().as_bytes(),
    )
    .unwrap()
}
