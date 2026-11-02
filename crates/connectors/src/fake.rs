//! The fake connector (feature `test-support`): a plain HTTP/1.1 server on `127.0.0.1:0` that
//! answers like `data.clauseo.chat` from the fixtures in `crates/connectors/fixtures/`, for this
//! crate's tests and R24's.
//!
//! Endpoints: the protected resource and authorization server metadata, the client document,
//! `/authorize` (no page: it checks the parameters and answers 302 to the `redirect_uri` with
//! `code`, `state` and `iss`), `/token` (both grants, codes single-use for 60 s, lazy refresh
//! rotation with a 30 s grace, reuse after it revokes the chain), `/revoke` (the whole chain, 200
//! for any token), `/mcp/case-law` (`initialize`, `server/discover`, `tools/list` and `ping`
//! without a token; `tools/call` needs one), `/v1/case-law/account`, `/v1/case-law/recharges`,
//! `/v1/case-law/recharges/{id}` (`wait` honoured up to [`FakeHandle::set_wait_cap`]),
//! `/pricing.json` and `/pay/{id}`. Every request is recorded ([`FakeHandle::requests`]).
//!
//! Its time is [`FakeOptions::clock`]: share a [`ManualClock`](crate::ManualClock) with the
//! [`Config`] to move both.

mod api;
mod client;
mod fixtures;
mod oauth;
mod server;

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::io;
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime};

use rand::RngCore as _;
use serde_json::Value;
use url::Url;

pub use self::client::{RawResponse, http};
pub use self::fixtures::fixture;
pub use crate::write::ScratchDir;

use crate::cancel::CancelFlag;
use crate::config::{
    Clock, Config, Origin, ProxyChoice, RECHARGE_DEADLINE, SIGNIN_TIMEOUT, SystemClock,
};

/// How the fake starts.
#[derive(Clone)]
pub struct FakeOptions {
    /// The fake's time: code expiry, access token expiry, the refresh grace.
    pub clock: Arc<dyn Clock>,
    /// The lifetime of the access tokens it issues (1 hour, as the connector's).
    pub access_ttl: Duration,
}

impl Default for FakeOptions {
    fn default() -> FakeOptions {
        FakeOptions {
            clock: Arc::new(SystemClock),
            access_ttl: Duration::from_secs(3600),
        }
    }
}

impl fmt::Debug for FakeOptions {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeOptions")
            .field("access_ttl", &self.access_ttl)
            .finish_non_exhaustive()
    }
}

/// An answer the fake gives from a fixture.
#[derive(Debug, Clone, PartialEq)]
pub enum Fixture {
    /// A file under `fixtures/` by its path there (`"pricing-preview.json"`), with the
    /// production origin replaced by the fake's; answered with 200.
    File(&'static str),
    /// This JSON, answered with 200.
    Json(Value),
    /// This status, `Content-Type` and body, as they are (a 500, a body that is not JSON).
    Raw {
        /// The HTTP status.
        status: u16,
        /// The `Content-Type`.
        content_type: String,
        /// The body.
        body: Vec<u8>,
    },
}

/// One request the fake received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordedRequest {
    /// `GET`, `POST`, ...
    pub method: String,
    /// The path, without the query.
    pub path: String,
    /// The raw query, without `?`; empty when there is none.
    pub query: String,
    /// The headers, names lowercased, in order.
    pub headers: Vec<(String, String)>,
    /// The body.
    pub body: Vec<u8>,
}

impl RecordedRequest {
    /// The first header named `name` (any case).
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(header, _)| *header == name)
            .map(|(_, value)| value.as_str())
    }

    /// The query's pairs, decoded, in order.
    pub fn query_pairs(&self) -> Vec<(String, String)> {
        pairs(self.query.as_bytes())
    }

    /// The body's form pairs (`application/x-www-form-urlencoded`), decoded, in order.
    pub fn form(&self) -> Vec<(String, String)> {
        pairs(&self.body)
    }

    /// The body as JSON, if it is.
    pub fn json(&self) -> Option<Value> {
        serde_json::from_slice(&self.body).ok()
    }
}

/// Form or query pairs, decoded.
fn pairs(bytes: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(bytes).into_owned().collect()
}

/// Starts fakes.
#[derive(Debug)]
pub struct FakeConnector;

impl FakeConnector {
    /// Binds `127.0.0.1:0` and serves until the handle drops.
    pub fn start(options: FakeOptions) -> io::Result<FakeHandle> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let base = format!("http://127.0.0.1:{port}");
        let base_url = Url::parse(&base).map_err(io::Error::other)?;
        let shared = Arc::new(Shared {
            base_url,
            state: Mutex::new(FakeState::new(&base, options.access_ttl)),
            base,
            clock: options.clock,
            paid: Condvar::new(),
            stop: CancelFlag::new(),
        });
        let accept = server::spawn(listener, Arc::clone(&shared))?;
        Ok(FakeHandle {
            shared,
            port,
            accept: Some(accept),
        })
    }
}

/// What the connection threads share.
struct Shared {
    /// `http://127.0.0.1:<port>`.
    base: String,
    base_url: Url,
    clock: Arc<dyn Clock>,
    state: Mutex<FakeState>,
    /// Woken when a recharge is paid, and on stop.
    paid: Condvar,
    stop: CancelFlag,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn now(&self) -> SystemTime {
        self.clock.now()
    }
}

/// Everything the fake knows.
struct FakeState {
    requests: Vec<RecordedRequest>,
    prm: Value,
    as_metadata: Value,
    client_metadata: Value,
    oauth: oauth::OAuthState,
    api: api::ApiState,
    fail_next: HashMap<String, VecDeque<u16>>,
    delay_next: HashMap<String, VecDeque<Duration>>,
}

impl FakeState {
    fn new(base: &str, access_ttl: Duration) -> FakeState {
        FakeState {
            requests: Vec::new(),
            prm: fixtures::value("prm-case-law.json", base),
            as_metadata: fixtures::value("as-metadata.json", base),
            client_metadata: fixtures::value("client-metadata.json", base),
            oauth: oauth::OAuthState::new(access_ttl),
            api: api::ApiState::new(),
            fail_next: HashMap::new(),
            delay_next: HashMap::new(),
        }
    }
}

/// `prefix` and 32 random hex characters.
fn random_token(prefix: &str) -> String {
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    format!("{prefix}{}", crate::secret::hex(&bytes).expose())
}

/// A running fake; it stops when this drops.
pub struct FakeHandle {
    shared: Arc<Shared>,
    port: u16,
    accept: Option<JoinHandle<()>>,
}

impl fmt::Debug for FakeHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("FakeHandle")
            .field("base", &self.shared.base)
            .finish_non_exhaustive()
    }
}

impl FakeHandle {
    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.shared.lock()
    }

    /// `http://127.0.0.1:<port>`, without a trailing `/`.
    pub fn base(&self) -> &str {
        &self.shared.base
    }

    /// [`FakeHandle::base`] as a URL, for `Origin::Fake`.
    pub fn base_url(&self) -> Url {
        self.shared.base_url.clone()
    }

    /// `base` joined with `path` (`"/pricing.json"`).
    pub fn url(&self, path: &str) -> Url {
        self.shared
            .base_url
            .join(path)
            .unwrap_or_else(|_| self.base_url())
    }

    /// A [`Config`] for this fake: `Origin::Fake`, user agent `Clauseo/0.0.0-test`, no proxy,
    /// the default timeouts, the fake's own clock, and `state_file` (a [`ScratchDir`]'s
    /// `state_file()`). Change any field after.
    pub fn config(&self, state_file: PathBuf) -> Config {
        Config {
            origin: Origin::Fake(self.base_url()),
            user_agent: "Clauseo/0.0.0-test".to_owned(),
            proxy: ProxyChoice::Direct,
            signin_timeout: SIGNIN_TIMEOUT,
            recharge_deadline: RECHARGE_DEADLINE,
            clock: Arc::clone(&self.shared.clock),
            state_file,
        }
    }

    /// The fake's clock.
    pub fn clock(&self) -> Arc<dyn Clock> {
        Arc::clone(&self.shared.clock)
    }

    /// Every request so far, oldest first.
    pub fn requests(&self) -> Vec<RecordedRequest> {
        self.lock().requests.clone()
    }

    /// The requests to `path` so far.
    pub fn requests_to(&self, path: &str) -> Vec<RecordedRequest> {
        self.lock()
            .requests
            .iter()
            .filter(|request| request.path == path)
            .cloned()
            .collect()
    }

    /// Forgets the requests so far.
    pub fn clear_requests(&self) {
        self.lock().requests.clear();
    }

    /// The next request to `path` (no query) answers `status` with
    /// `{"error":{"code":"injected","status":<status>}}`. Calls queue up.
    pub fn fail_next(&self, path: &str, status: u16) {
        self.lock()
            .fail_next
            .entry(path.to_owned())
            .or_default()
            .push_back(status);
    }

    /// The next request to `path` is answered after `delay` of real time (cut short when the
    /// fake stops). Calls queue up.
    pub fn delay_next(&self, path: &str, delay: Duration) {
        self.lock()
            .delay_next
            .entry(path.to_owned())
            .or_default()
            .push_back(delay);
    }

    /// The protected resource metadata served now (origin already swapped).
    pub fn prm(&self) -> Value {
        self.lock().prm.clone()
    }

    /// Serves `value` as the protected resource metadata.
    pub fn set_prm(&self, value: Value) {
        self.lock().prm = value;
    }

    /// The authorization server metadata served now (origin already swapped).
    pub fn as_metadata(&self) -> Value {
        self.lock().as_metadata.clone()
    }

    /// Serves `value` as the authorization server metadata.
    pub fn set_as_metadata(&self, value: Value) {
        self.lock().as_metadata = value;
    }

    /// What `/pricing.json` answers (`Fixture::File("pricing-live.json")` at start).
    pub fn set_pricing(&self, fixture: Fixture) {
        self.lock().api.pricing = fixture;
    }

    /// The credit balance (1000 at start). At 0 or below, every paid `tools/call` answers the
    /// out-of-credit result, and `GET /v1/case-law/account` and `POST /v1/case-law/recharges`
    /// answer 402 with `api-402.json`'s body; each opens a recharge (or reuses the open one).
    pub fn set_balance(&self, credits: i64) {
        self.lock().api.balance = credits;
    }

    /// The credit balance now.
    pub fn balance(&self) -> i64 {
        self.lock().api.balance
    }

    /// The lifetime of access tokens issued from now on.
    pub fn set_access_ttl(&self, ttl: Duration) {
        self.lock().oauth.access_ttl = ttl;
    }

    /// Every access token issued so far is expired; refresh tokens stay valid.
    pub fn expire_access_tokens(&self) {
        self.lock().oauth.expire_access_tokens();
    }

    /// Revokes every token and API key issued so far.
    pub fn revoke_all(&self) {
        self.lock().oauth.revoke_all();
    }

    /// Every authorization code issued so far is stale, as if 60 s had passed.
    pub fn expire_codes(&self) {
        self.lock().oauth.expire_codes();
    }

    /// `/authorize` leaves `iss` out of its redirect from now on.
    pub fn omit_iss(&self) {
        self.lock().oauth.omit_iss = true;
    }

    /// `/authorize` sends this `iss` from now on.
    pub fn wrong_iss(&self, iss: &str) {
        self.lock().oauth.wrong_iss = Some(iss.to_owned());
    }

    /// `/authorize` redirects with `error=<error>` (`access_denied`) instead of a code, from now
    /// on; `None` goes back to codes.
    pub fn deny_authorize(&self, error: Option<&str>) {
        self.lock().oauth.deny = error.map(str::to_owned);
    }

    /// A new valid `cdk_live_` API key.
    pub fn new_api_key(&self) -> String {
        let key = oauth::new_api_key();
        self.add_api_key(&key);
        key
    }

    /// Accepts `key` as a bearer token.
    pub fn add_api_key(&self, key: &str) {
        let mut state = self.lock();
        state.oauth.api_keys.insert(key.to_owned());
        state.oauth.issued.push(key.to_owned());
    }

    /// Every code, access token, refresh token and API key the fake issued or accepted, for leak
    /// checks. The code verifiers are in the recorded `/token` forms.
    pub fn issued_secrets(&self) -> Vec<String> {
        self.lock().oauth.issued.clone()
    }

    /// The next `tools/call` of `name` answers `fixture` (a result object, or a raw HTTP
    /// answer); calls queue up per tool and take precedence over the balance.
    pub fn push_tool_result(&self, name: &str, fixture: Fixture) {
        self.lock()
            .api
            .tool_results
            .entry(name.to_owned())
            .or_default()
            .push_back(fixture);
    }

    /// Marks a recharge paid, adds its credits, and wakes any `wait` poll on it. False for an
    /// unknown id.
    pub fn mark_paid(&self, recharge_id: &str) -> bool {
        let paid = self.lock().api.mark_paid(recharge_id);
        self.shared.paid.notify_all();
        paid
    }

    /// The ids of every recharge opened so far, oldest first.
    pub fn recharges(&self) -> Vec<String> {
        self.lock().api.recharge_ids()
    }

    /// MCP answers come as `text/event-stream` (one `message` event) instead of JSON.
    pub fn respond_as_sse(&self, sse: bool) {
        self.lock().api.sse = sse;
    }

    /// The longest a `wait=N` recharge poll holds the answer (2 s at start).
    pub fn set_wait_cap(&self, cap: Duration) {
        self.lock().api.wait_cap = cap;
    }

    /// Drives the browser's part of a sign-in: `GET authorize_url` at the fake, then, when it
    /// answers 302, `GET` its `Location` (the client's loopback callback). Returns the callback's
    /// answer, or the fake's when it did not redirect. An authorize URL on another origin is
    /// refused.
    pub fn drive_authorize(&self, authorize_url: &Url) -> io::Result<RawResponse> {
        if authorize_url.origin() != self.shared.base_url.origin() {
            return Err(io::Error::other("the authorize URL is not this fake's"));
        }
        let first = http("GET", authorize_url, &[], &[])?;
        if first.status != 302 {
            return Ok(first);
        }
        let location = first
            .header("location")
            .ok_or_else(|| io::Error::other("302 without Location"))?;
        let callback = Url::parse(location).map_err(io::Error::other)?;
        http("GET", &callback, &[], &[])
    }
}

impl Drop for FakeHandle {
    fn drop(&mut self) {
        self.shared.stop.cancel();
        self.shared.paid.notify_all();
        // Wakes the accept loop, which sees the stop flag and ends.
        drop(TcpStream::connect((Ipv4Addr::LOCALHOST, self.port)));
        if let Some(accept) = self.accept.take()
            && accept.join().is_err()
        {
            tracing::warn!("the fake connector's accept thread panicked");
        }
    }
}
