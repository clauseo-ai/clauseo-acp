//! The OAuth sign-in, steps 1 to 9 of the brief: discovery with every check, PKCE S256 and
//! `state`, the loopback listener, the authorize URL, the callback's checks, the token exchange,
//! and the credential stored in the keychain and then the state file.
//!
//! Discovery's metadata is kept in memory for the process (per [`ConnectorHandle`]); the
//! refresh and the revoke read their endpoints from it.

use std::fmt;
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::Ordering;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use rand::RngCore as _;
use serde::Deserialize;
use serde_json::Value;
use url::Url;
use zeroize::Zeroizing;

use crate::cancel::CancelFlag;
use crate::connectors::{ConnectorHandle, lock};
use crate::credential::{Credential, CredentialKind, OAuthTokens, WireSecret};
use crate::error::SignInError;
use crate::http::{self, SendBody};
use crate::net::bind_loopback_exclusive;
use crate::pkce::s256_challenge;
use crate::secret::Secret;

mod listener;

pub(crate) use listener::ListenerStats;

/// The page the browser shows once the credential is stored: English, then Hindi.
pub(crate) const SUCCESS_PAGE: &str = include_str!("oauth/page.html");
/// The page the browser shows, with 400, when the callback or the exchange failed: English,
/// then Hindi.
pub(crate) const FAILURE_PAGE: &str = include_str!("oauth/failed.html");

/// The endpoints discovery found, each `https` on the issuer's host (the fake's own origin in
/// tests).
#[derive(Debug)]
pub(crate) struct Metadata {
    pub(crate) authorize: Url,
    pub(crate) token: Url,
    pub(crate) revocation: Url,
}

/// A sign-in waiting for the browser. Give [`PendingSignIn::authorize_url`] to the system
/// browser, then call [`PendingSignIn::finish`]. Dropping it closes the listener.
pub struct PendingSignIn {
    handle: ConnectorHandle,
    metadata: Arc<Metadata>,
    listener: TcpListener,
    authorize_url: Url,
    redirect_uri: String,
    verifier: Secret,
    state: Secret,
    cancel: CancelFlag,
    stats: Arc<ListenerStats>,
    _pending: PendingGuard,
}

impl fmt::Debug for PendingSignIn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingSignIn")
            .field("connector", &self.handle.known().id)
            .field("redirect_uri", &self.redirect_uri)
            .finish_non_exhaustive()
    }
}

/// Clears the handle's pending sign-in when this one ends, unless a newer one replaced it.
struct PendingGuard {
    handle: ConnectorHandle,
    number: u64,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        let mut pending = lock(&self.handle.inner.pending);
        if pending
            .as_ref()
            .is_some_and(|(number, _)| *number == self.number)
        {
            *pending = None;
        }
    }
}

impl PendingSignIn {
    /// The URL to open in the system browser. It holds no secret: the `state` and the PKCE
    /// challenge only.
    pub fn authorize_url(&self) -> &Url {
        &self.authorize_url
    }

    /// A flag that ends [`PendingSignIn::finish`] with `Cancelled`.
    pub fn cancel_flag(&self) -> CancelFlag {
        self.cancel.clone()
    }

    /// Waits for the browser's callback, the sign-in timeout or the cancel flag; then exchanges
    /// the code and stores the credential (the keychain, then the state file), and only then
    /// answers the browser: the success page, or the failure page with 400.
    pub fn finish(self) -> Result<(), SignInError> {
        let PendingSignIn {
            handle,
            metadata,
            listener,
            redirect_uri,
            verifier,
            state,
            cancel,
            stats,
            _pending,
            ..
        } = self;
        let config = &handle.inner.shared.config;
        let deadline = config
            .clock
            .now()
            .checked_add(config.signin_timeout)
            .ok_or_else(|| {
                SignInError::Exchange("the sign-in timeout is out of range".to_owned())
            })?;
        let expect = listener::Expect {
            state: &state,
            issuer: &handle.inner.urls.issuer,
        };
        // The listener is moved in and closed as soon as the callback is answered.
        let (code, browser) =
            listener::wait_for_code(listener, &expect, &cancel, &*config.clock, deadline, &stats)?;
        if cancel.is_cancelled() {
            browser.failed();
            return Err(SignInError::Cancelled);
        }
        match exchange(&handle, &metadata, &code, &redirect_uri, &verifier) {
            Ok(()) => {
                browser.signed_in();
                Ok(())
            }
            Err(err) => {
                browser.failed();
                Err(err)
            }
        }
    }

    /// Replaces the PKCE verifier with another valid one, so the exchange fails as a stolen
    /// code would.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn tamper_verifier_for_test(&mut self) {
        self.verifier = Secret::from("A".repeat(43).as_str());
    }
}

/// Starts a sign-in: discovery (checked before any listener opens), PKCE and `state`, the
/// loopback listener on a free port, and the authorize URL. A pending sign-in of this
/// connector is cancelled.
pub(crate) fn begin_sign_in(
    handle: &ConnectorHandle,
    login_hint: Option<&str>,
) -> Result<PendingSignIn, SignInError> {
    let metadata = metadata(handle)?;
    let verifier = Secret::new(URL_SAFE_NO_PAD.encode(*random::<32>(handle)));
    let state = Secret::new(URL_SAFE_NO_PAD.encode(*random::<16>(handle)));
    let challenge = s256_challenge(verifier.expose());
    let cancel = CancelFlag::new();
    let number = handle.inner.next_sign_in.fetch_add(1, Ordering::Relaxed);
    if let Some((_, earlier)) = lock(&handle.inner.pending).replace((number, cancel.clone())) {
        earlier.cancel();
    }
    let pending = PendingGuard {
        handle: handle.clone(),
        number,
    };
    let listener_failed =
        |err: std::io::Error| SignInError::Exchange(format!("loopback listener: {:?}", err.kind()));
    #[cfg(feature = "test-support")]
    handle.inner.listeners_bound.fetch_add(1, Ordering::SeqCst);
    let listener = bind_loopback_exclusive(0).map_err(listener_failed)?;
    listener.set_nonblocking(true).map_err(listener_failed)?;
    let port = listener.local_addr().map_err(listener_failed)?.port();
    let redirect_uri = format!("http://127.0.0.1:{port}/oauth/callback");
    let stats = Arc::new(ListenerStats::default());
    *lock(&handle.inner.listener_stats) = Some(Arc::clone(&stats));
    let urls = &handle.inner.urls;
    let mut authorize_url = metadata.authorize.clone();
    {
        let mut query = authorize_url.query_pairs_mut();
        query
            .append_pair("response_type", "code")
            .append_pair("client_id", &urls.client_id)
            .append_pair("redirect_uri", &redirect_uri)
            .append_pair("scope", handle.inner.known.scope)
            .append_pair("state", state.expose())
            .append_pair("code_challenge", &challenge)
            .append_pair("code_challenge_method", "S256");
        for resource in &urls.resources {
            query.append_pair("resource", resource);
        }
        if let Some(hint) = login_hint {
            query.append_pair("login_hint", hint);
        }
    }
    Ok(PendingSignIn {
        handle: handle.clone(),
        metadata,
        listener,
        authorize_url,
        redirect_uri,
        verifier,
        state,
        cancel,
        stats,
        _pending: pending,
    })
}

/// Seeds the generator of the verifier and `state`, for the authorize URL snapshot.
#[cfg(feature = "test-support")]
#[doc(hidden)]
pub(crate) fn seed_rng_for_test(handle: &ConnectorHandle, seed: u64) {
    use rand::SeedableRng as _;
    *lock(&handle.inner.test_rng) = Some(rand::rngs::StdRng::seed_from_u64(seed));
}

/// `N` random bytes from the OS-seeded generator, zeroed on drop.
fn random<const N: usize>(handle: &ConnectorHandle) -> Zeroizing<[u8; N]> {
    let mut bytes = Zeroizing::new([0u8; N]);
    #[cfg(feature = "test-support")]
    if let Some(rng) = lock(&handle.inner.test_rng).as_mut() {
        rng.fill_bytes(&mut *bytes);
        return bytes;
    }
    // Without test-support the handle has no seeded generator to offer.
    #[cfg(not(feature = "test-support"))]
    let _ = handle;
    rand::rng().fill_bytes(&mut *bytes);
    bytes
}

/// The discovery metadata, fetched and checked once per process.
pub(crate) fn metadata(handle: &ConnectorHandle) -> Result<Arc<Metadata>, SignInError> {
    let mut cached = lock(&handle.inner.metadata);
    if let Some(metadata) = cached.as_ref() {
        return Ok(Arc::clone(metadata));
    }
    let metadata = Arc::new(discover(handle)?);
    *cached = Some(Arc::clone(&metadata));
    Ok(metadata)
}

/// Steps 2 and 3: the protected resource metadata, then the authorization server's.
fn discover(handle: &ConnectorHandle) -> Result<Metadata, SignInError> {
    let urls = &handle.inner.urls;
    let what = "protected resource metadata";
    let prm = get_json(handle, &urls.protected_resource_metadata, what)?;
    if prm.get("resource").and_then(Value::as_str) != Some(urls.mcp_url.as_str()) {
        return Err(refused(what, "resource is not the MCP URL"));
    }
    if !list_has(&prm, "authorization_servers", &urls.issuer) {
        return Err(refused(what, "authorization_servers lacks the issuer"));
    }
    let what = "authorization server metadata";
    let asm = get_json(handle, &urls.authorization_server_metadata, what)?;
    if asm.get("issuer").and_then(Value::as_str) != Some(urls.issuer.as_str()) {
        return Err(refused(what, "issuer differs"));
    }
    for flag in [
        "client_id_metadata_document_supported",
        "authorization_response_iss_parameter_supported",
    ] {
        if asm.get(flag).and_then(Value::as_bool) != Some(true) {
            return Err(refused(what, &format!("{flag} is not true")));
        }
    }
    if !list_has(&asm, "token_endpoint_auth_methods_supported", "none") {
        return Err(refused(what, "token endpoint auth method none missing"));
    }
    if !list_has(&asm, "code_challenge_methods_supported", "S256") {
        return Err(refused(what, "code challenge method S256 missing"));
    }
    Ok(Metadata {
        authorize: endpoint(handle, &asm, "authorization_endpoint")?,
        token: endpoint(handle, &asm, "token_endpoint")?,
        revocation: endpoint(handle, &asm, "revocation_endpoint")?,
    })
}

/// An endpoint of the metadata, which must be on the issuer's scheme, host and port (so
/// `https` and `data.clauseo.chat` in production), with no user info or fragment.
fn endpoint(handle: &ConnectorHandle, asm: &Value, name: &str) -> Result<Url, SignInError> {
    let what = "authorization server metadata";
    let text = asm
        .get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| refused(what, &format!("{name} missing")))?;
    let url =
        Url::parse(text).map_err(|_not_url| refused(what, &format!("{name} is not a URL")))?;
    let issuer = Url::parse(&handle.inner.urls.issuer)
        .map_err(|_not_url| refused(what, "the issuer is not a URL"))?;
    let ours = url.scheme() == issuer.scheme()
        && url.origin() == issuer.origin()
        && url.username().is_empty()
        && url.password().is_none()
        && url.fragment().is_none();
    if ours {
        Ok(url)
    } else {
        Err(refused(
            what,
            &format!("{name} is not on the issuer's host"),
        ))
    }
}

/// A metadata document: 200 and a JSON object.
fn get_json(handle: &ConnectorHandle, url: &str, what: &str) -> Result<Value, SignInError> {
    let shared = &handle.inner.shared;
    let reply = http::send(
        &shared.agent,
        "GET",
        url,
        &[("Accept", "application/json")],
        SendBody::None,
        shared.timeout(),
    )
    .map_err(|err| refused(what, &err))?;
    if reply.status != 200 {
        return Err(refused(what, &format!("HTTP {}", reply.status)));
    }
    match serde_json::from_slice::<Value>(&reply.body) {
        Ok(value) if value.is_object() => Ok(value),
        _ => Err(refused(what, "not a JSON object")),
    }
}

/// Steps 8 and 9: the code for tokens (both resources, no secret), then the keychain, then
/// the state file.
fn exchange(
    handle: &ConnectorHandle,
    metadata: &Metadata,
    code: &Secret,
    redirect_uri: &str,
    verifier: &Secret,
) -> Result<(), SignInError> {
    let urls = &handle.inner.urls;
    let mut pairs = vec![
        ("grant_type", "authorization_code"),
        ("code", code.expose()),
        ("redirect_uri", redirect_uri),
        ("client_id", urls.client_id.as_str()),
        ("code_verifier", verifier.expose()),
    ];
    pairs.extend(
        urls.resources
            .iter()
            .map(|resource| ("resource", resource.as_str())),
    );
    let form = encode_form(&pairs);
    let shared = &handle.inner.shared;
    let reply = http::send(
        &shared.agent,
        "POST",
        metadata.token.as_str(),
        &[("Accept", "application/json")],
        SendBody::Form(&form),
        shared.timeout(),
    )
    .map_err(|err| SignInError::Exchange(format!("network: {err}")))?;
    if reply.status != 200 {
        return Err(SignInError::Exchange(format!(
            "the token endpoint answered HTTP {} ({})",
            reply.status,
            oauth_error_code(&reply.body)
        )));
    }
    let tokens =
        parse_token_answer(&reply.body, shared.unix_now(), None).map_err(SignInError::Exchange)?;
    handle.store_new(Credential::OAuth(tokens), CredentialKind::OAuth)?;
    Ok(())
}

fn refused(what: &str, why: &str) -> SignInError {
    SignInError::Discovery(format!("{what}: {why}"))
}

/// Whether `value[name]` is an array holding the string `item`.
fn list_has(value: &Value, name: &str, item: &str) -> bool {
    value
        .get(name)
        .and_then(Value::as_array)
        .is_some_and(|list| list.iter().any(|entry| entry.as_str() == Some(item)))
}

/// A form body in a buffer zeroed on drop, sized so it never grows (percent-encoding at most
/// triples a byte), so no unzeroed copy is left behind.
pub(crate) fn encode_form(pairs: &[(&str, &str)]) -> Zeroizing<Vec<u8>> {
    let size = pairs
        .iter()
        .map(|(name, value)| name.len().saturating_add(value.len()).saturating_mul(3) + 2)
        .sum::<usize>();
    let mut text = Zeroizing::new(String::with_capacity(size));
    url::form_urlencoded::Serializer::new(&mut *text)
        .extend_pairs(pairs)
        .finish();
    Zeroizing::new(text.as_bytes().to_vec())
}

/// An RFC 6749 section 5.2 `error` code from a token answer, if it looks like one; never the
/// description.
pub(crate) fn oauth_error_code(body: &[u8]) -> String {
    let code = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| {
            value
                .get("error")
                .and_then(Value::as_str)
                .map(str::to_owned)
        });
    match code {
        Some(code) if is_error_code(&code) => code,
        _ => "no error code".to_owned(),
    }
}

/// `[a-z0-9_]{1,64}`: an OAuth error code, safe to log.
pub(crate) fn is_error_code(code: &str) -> bool {
    (1..=64).contains(&code.len())
        && code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
}

#[derive(Deserialize)]
struct TokenIn {
    access_token: Option<WireSecret>,
    token_type: Option<String>,
    expires_in: Option<Value>,
    refresh_token: Option<WireSecret>,
}

/// A token answer (step 8, or a refresh): `access_token`, `token_type` (`bearer`, any case),
/// `expires_in` and `refresh_token`. A refresh answer without `refresh_token` keeps
/// `previous`. The error names the missing part, never a value.
pub(crate) fn parse_token_answer(
    body: &[u8],
    now_unix: u64,
    previous: Option<&Secret>,
) -> Result<OAuthTokens, String> {
    let answer: TokenIn = serde_json::from_slice(body)
        .map_err(|err| format!("the token answer did not parse ({:?})", err.classify()))?;
    if !answer
        .token_type
        .is_some_and(|kind| kind.eq_ignore_ascii_case("bearer"))
    {
        return Err("the token type is not bearer".to_owned());
    }
    let access_token = answer
        .access_token
        .map(|token| token.0)
        .filter(|token| !token.is_empty())
        .ok_or_else(|| "the token answer has no access_token".to_owned())?;
    let expires_in = answer
        .expires_in
        .and_then(|value| {
            value
                .as_u64()
                .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
        })
        .ok_or_else(|| "the token answer has no expires_in".to_owned())?;
    let refresh_token = match answer.refresh_token.map(|token| token.0) {
        Some(token) if !token.is_empty() => token,
        _ => previous
            .cloned()
            .ok_or_else(|| "the token answer has no refresh_token".to_owned())?,
    };
    Ok(OAuthTokens {
        access_token,
        access_expires_at: now_unix.saturating_add(expires_in),
        refresh_token,
    })
}

#[cfg(test)]
mod tests;
