//! The fake's authorization server: `/authorize`, `/token` and `/revoke`, and the bearer check
//! of the API and `tools/call`.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, SystemTime};

use rand::Rng as _;
use serde_json::json;
use url::Url;

use super::server::Response;
use super::{RecordedRequest, Shared, random_token};
use crate::pkce::s256_challenge;

/// How long a code stays usable.
const CODE_TTL: Duration = Duration::from_secs(60);
/// How long an old refresh token stays usable after its successor's first use.
const ROTATION_GRACE: Duration = Duration::from_secs(30);
/// The scope the desktop client must ask for.
const SCOPE: [&str; 2] = ["case-law", "offline_access"];

/// The tokens, codes and API keys the fake issued, and the `/authorize` knobs.
pub(super) struct OAuthState {
    codes: HashMap<String, Code>,
    access: HashMap<String, Access>,
    refresh: HashMap<String, Refresh>,
    revoked: HashSet<u64>,
    next_chain: u64,
    pub(super) access_ttl: Duration,
    pub(super) api_keys: HashSet<String>,
    pub(super) omit_iss: bool,
    pub(super) wrong_iss: Option<String>,
    pub(super) deny: Option<String>,
    /// Every secret issued or accepted, for leak checks.
    pub(super) issued: Vec<String>,
}

struct Code {
    challenge: String,
    redirect_uri: String,
    issued_at: SystemTime,
    stale: bool,
    /// The chain the code was exchanged for, once it was.
    chain: Option<u64>,
}

struct Access {
    chain: u64,
    expires_at: SystemTime,
}

struct Refresh {
    chain: u64,
    predecessor: Option<String>,
    successor: Option<String>,
    first_used: Option<SystemTime>,
    successor_first_used: Option<SystemTime>,
}

impl OAuthState {
    pub(super) fn new(access_ttl: Duration) -> OAuthState {
        OAuthState {
            codes: HashMap::new(),
            access: HashMap::new(),
            refresh: HashMap::new(),
            revoked: HashSet::new(),
            next_chain: 1,
            access_ttl,
            api_keys: HashSet::new(),
            omit_iss: false,
            wrong_iss: None,
            deny: None,
            issued: Vec::new(),
        }
    }

    pub(super) fn expire_access_tokens(&mut self) {
        for access in self.access.values_mut() {
            access.expires_at = SystemTime::UNIX_EPOCH;
        }
    }

    pub(super) fn revoke_all(&mut self) {
        let chains: Vec<u64> = self.refresh.values().map(|refresh| refresh.chain).collect();
        self.revoked.extend(chains);
        let chains: Vec<u64> = self.access.values().map(|access| access.chain).collect();
        self.revoked.extend(chains);
        self.api_keys.clear();
    }

    pub(super) fn expire_codes(&mut self) {
        for code in self.codes.values_mut() {
            code.stale = true;
        }
    }

    /// Whether an `Authorization` header carries a live access token or a known API key.
    pub(super) fn bearer_ok(&self, authorization: Option<&str>, now: SystemTime) -> bool {
        let Some(token) = authorization.and_then(bearer_token) else {
            return false;
        };
        if self.api_keys.contains(token) {
            return true;
        }
        self.access
            .get(token)
            .is_some_and(|access| !self.revoked.contains(&access.chain) && now < access.expires_at)
    }

    /// A new access token and `refresh` (already stored) for `chain`, as the token answer.
    fn answer(&mut self, chain: u64, refresh: &str, now: SystemTime) -> Response {
        let access = random_token("fake_at_");
        self.issued.push(access.clone());
        self.access.insert(
            access.clone(),
            Access {
                chain,
                expires_at: now + self.access_ttl,
            },
        );
        Response::json(
            200,
            &json!({
                "access_token": access,
                "token_type": "Bearer",
                "expires_in": self.access_ttl.as_secs(),
                "refresh_token": refresh,
                "scope": SCOPE.join(" "),
            }),
        )
        .with("Cache-Control", "no-store")
    }

    /// A new refresh token in `chain`, after `predecessor`.
    fn new_refresh(&mut self, chain: u64, predecessor: Option<String>) -> String {
        let token = random_token("fake_rt_");
        self.issued.push(token.clone());
        self.refresh.insert(
            token.clone(),
            Refresh {
                chain,
                predecessor,
                successor: None,
                first_used: None,
                successor_first_used: None,
            },
        );
        token
    }
}

/// The token of `Bearer <token>` (the scheme in any case).
fn bearer_token(header: &str) -> Option<&str> {
    let (scheme, token) = header.split_once(' ')?;
    scheme.eq_ignore_ascii_case("bearer").then(|| token.trim())
}

/// `cdk_live_` and 32 random base62 characters.
pub(super) fn new_api_key() -> String {
    const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::rng();
    let tail: String = (0..32)
        .map(|_| char::from(BASE62[rng.random_range(0..BASE62.len())]))
        .collect();
    format!("cdk_live_{tail}")
}

/// The values of `name` among `pairs`.
fn all<'a>(pairs: &'a [(String, String)], name: &str) -> Vec<&'a str> {
    pairs
        .iter()
        .filter(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
        .collect()
}

/// The value of `name` when it appears exactly once.
fn one<'a>(pairs: &'a [(String, String)], name: &str) -> Option<&'a str> {
    match all(pairs, name).as_slice() {
        [value] => Some(value),
        _ => None,
    }
}

/// Whether `resources` are exactly the connector's two, MCP and API, each once.
fn resources_ok(shared: &Shared, resources: &[&str]) -> bool {
    let mcp = format!("{}/mcp/case-law", shared.base);
    let api = format!("{}/v1/case-law", shared.base);
    let mut got = resources.to_vec();
    got.sort_unstable();
    let mut want = vec![mcp.as_str(), api.as_str()];
    want.sort_unstable();
    got == want
}

/// `http://127.0.0.1:<port>/oauth/callback`, any port, no query or fragment.
fn redirect_ok(redirect_uri: &str) -> bool {
    Url::parse(redirect_uri).is_ok_and(|url| {
        url.scheme() == "http"
            && url.host_str() == Some("127.0.0.1")
            && url.port().is_some()
            && url.path() == "/oauth/callback"
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}

/// An RFC 6749 section 5.2 error.
fn oauth_error(status: u16, error: &str, description: &str) -> Response {
    Response::json(
        status,
        &json!({"error": error, "error_description": description}),
    )
    .with("Cache-Control", "no-store")
}

/// `GET /authorize`: checks every parameter, then 302 to the `redirect_uri` with `code`,
/// `state` and `iss`. A bad parameter is a 400 naming it (no redirect: the fake has no page).
pub(super) fn authorize(shared: &Shared, request: &RecordedRequest) -> Response {
    let params = request.query_pairs();
    let client_id = format!("{}/oauth/clients/clauseo-desktop.json", shared.base);
    if one(&params, "client_id") != Some(client_id.as_str()) {
        return Response::text(400, "client_id");
    }
    let Some(redirect_uri) = one(&params, "redirect_uri").filter(|uri| redirect_ok(uri)) else {
        return Response::text(400, "redirect_uri");
    };
    if one(&params, "response_type") != Some("code") {
        return Response::text(400, "response_type");
    }
    if one(&params, "code_challenge_method") != Some("S256") {
        return Response::text(400, "code_challenge_method");
    }
    let Some(challenge) = one(&params, "code_challenge").filter(|challenge| {
        challenge.len() == 43
            && challenge
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    }) else {
        return Response::text(400, "code_challenge");
    };
    let mut scope: Vec<&str> = one(&params, "scope")
        .unwrap_or_default()
        .split(' ')
        .collect();
    scope.sort_unstable();
    if scope != SCOPE {
        return Response::text(400, "scope");
    }
    if !resources_ok(shared, &all(&params, "resource")) {
        return Response::text(400, "resource");
    }
    let Some(state_param) = one(&params, "state").filter(|state| !state.is_empty()) else {
        return Response::text(400, "state");
    };
    let mut state = shared.lock();
    let Ok(mut location) = Url::parse(redirect_uri) else {
        return Response::text(400, "redirect_uri");
    };
    {
        let mut query = location.query_pairs_mut();
        if let Some(error) = &state.oauth.deny {
            query.append_pair("error", error);
        } else {
            let code = random_token("fake_code_");
            state.oauth.issued.push(code.clone());
            state.oauth.codes.insert(
                code.clone(),
                Code {
                    challenge: challenge.to_owned(),
                    redirect_uri: redirect_uri.to_owned(),
                    issued_at: shared.now(),
                    stale: false,
                    chain: None,
                },
            );
            query.append_pair("code", &code);
        }
        query.append_pair("state", state_param);
        if !state.oauth.omit_iss {
            let iss = state.oauth.wrong_iss.clone();
            query.append_pair("iss", iss.as_deref().unwrap_or(&shared.base));
        }
    }
    Response::empty(302)
        .with("Location", location.as_str())
        .with("Cache-Control", "no-store")
}

/// `POST /token`: `authorization_code` and `refresh_token` grants.
pub(super) fn token(shared: &Shared, request: &RecordedRequest) -> Response {
    let form_type = request
        .header("content-type")
        .is_some_and(|value| value.starts_with("application/x-www-form-urlencoded"));
    if !form_type {
        return oauth_error(400, "invalid_request", "the body must be a form");
    }
    let params = request.form();
    let client_id = format!("{}/oauth/clients/clauseo-desktop.json", shared.base);
    if one(&params, "client_id") != Some(client_id.as_str()) {
        return oauth_error(401, "invalid_client", "unknown client_id");
    }
    match one(&params, "grant_type") {
        Some("authorization_code") => exchange_code(shared, &params),
        Some("refresh_token") => refresh(shared, &params),
        _ => oauth_error(400, "unsupported_grant_type", "unsupported grant_type"),
    }
}

fn exchange_code(shared: &Shared, params: &[(String, String)]) -> Response {
    let now = shared.now();
    let mut state = shared.lock();
    let oauth = &mut state.oauth;
    let Some(code) = one(params, "code").and_then(|code| oauth.codes.get_mut(code)) else {
        return oauth_error(400, "invalid_grant", "unknown code");
    };
    if let Some(chain) = code.chain {
        // A reused code revokes what it was exchanged for (RFC 6749 section 4.1.2).
        oauth.revoked.insert(chain);
        return oauth_error(400, "invalid_grant", "the code was used");
    }
    if code.stale
        || code
            .issued_at
            .checked_add(CODE_TTL)
            .is_none_or(|end| end < now)
    {
        return oauth_error(400, "invalid_grant", "the code expired");
    }
    // The code is spent now, whatever the checks below say; its chain number is reserved.
    let chain = oauth.next_chain;
    oauth.next_chain = oauth.next_chain.saturating_add(1);
    code.chain = Some(chain);
    if one(params, "redirect_uri") != Some(code.redirect_uri.as_str()) {
        return oauth_error(400, "invalid_grant", "redirect_uri differs");
    }
    let verifier_ok = one(params, "code_verifier").is_some_and(|verifier| {
        (43..=128).contains(&verifier.len()) && s256_challenge(verifier) == code.challenge
    });
    if !verifier_ok {
        return oauth_error(400, "invalid_grant", "the code_verifier does not match");
    }
    if !resources_ok(shared, &all(params, "resource")) {
        return oauth_error(400, "invalid_target", "both resources are required");
    }
    let refresh = oauth.new_refresh(chain, None);
    oauth.answer(chain, &refresh, now)
}

fn refresh(shared: &Shared, params: &[(String, String)]) -> Response {
    let now = shared.now();
    let resources = all(params, "resource");
    if !resources.is_empty() && !resources_ok(shared, &resources) {
        return oauth_error(400, "invalid_target", "both resources, or none");
    }
    let mut state = shared.lock();
    let oauth = &mut state.oauth;
    let Some(token) = one(params, "refresh_token").map(str::to_owned) else {
        return oauth_error(400, "invalid_request", "no refresh_token");
    };
    let Some(presented) = oauth.refresh.get_mut(&token) else {
        return oauth_error(400, "invalid_grant", "unknown refresh token");
    };
    let chain = presented.chain;
    if oauth.revoked.contains(&chain) {
        return oauth_error(400, "invalid_grant", "the refresh token was revoked");
    }
    let first_use = presented.first_used.is_none();
    presented.first_used.get_or_insert(now);
    let predecessor = presented.predecessor.clone();
    let successor = presented.successor.clone();
    let successor_first_used = presented.successor_first_used;
    if first_use
        && let Some(predecessor) = predecessor.and_then(|token| oauth.refresh.get_mut(&token))
    {
        predecessor.successor_first_used = Some(now);
    }
    match successor {
        // Rotation: the presented token gets a successor and stays valid until 30 s after the
        // successor's first use.
        None => {
            let next = oauth.new_refresh(chain, Some(token.clone()));
            if let Some(presented) = oauth.refresh.get_mut(&token) {
                presented.successor = Some(next.clone());
            }
            oauth.answer(chain, &next, now)
        }
        Some(next) => match successor_first_used {
            Some(used) if used.checked_add(ROTATION_GRACE).is_none_or(|end| end < now) => {
                oauth.revoked.insert(chain);
                oauth_error(400, "invalid_grant", "a rotated refresh token was reused")
            }
            // Within the grace, or the successor never used: the same successor again, so the
            // chain never forks.
            _ => oauth.answer(chain, &next, now),
        },
    }
}

/// `POST /revoke` (RFC 7009): revokes the token's whole chain, or the API key; 200 for any
/// token.
pub(super) fn revoke(shared: &Shared, request: &RecordedRequest) -> Response {
    let params = request.form();
    let mut state = shared.lock();
    let oauth = &mut state.oauth;
    if let Some(token) = one(&params, "token") {
        let chain = oauth
            .refresh
            .get(token)
            .map(|refresh| refresh.chain)
            .or_else(|| oauth.access.get(token).map(|access| access.chain));
        if let Some(chain) = chain {
            oauth.revoked.insert(chain);
        }
        oauth.api_keys.remove(token);
    }
    Response::empty(200).with("Cache-Control", "no-store")
}
