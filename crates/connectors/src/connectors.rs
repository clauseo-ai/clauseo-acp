//! [`Connectors`], one per app, and [`ConnectorHandle`], one per known connector: the credential
//! in memory after one keychain read, the state file, liveness, the API key, sign-out and
//! removal. The sign-in is in `oauth.rs`, the refresh in `refresh.rs`, the calls in `calls.rs`.
//!
//! Locks: a handle's credential slot is taken before the shared state; nothing takes them the
//! other way round. The account cache is taken before the slot.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use zeroize::Zeroizing;

use crate::account::Account;
use crate::cancel::CancelFlag;
use crate::config::{Config, ConnectorUrls};
use crate::credential::{Credential, CredentialKind, CredentialStatus, SignOutReport};
use crate::error::{AuthError, Error, SignInError, StoreError};
use crate::http::{self, SendBody};
use crate::known::{CASE_LAW, KNOWN, KnownConnector};
use crate::mcp::{McpReply, McpRequestHeaders};
use crate::oauth::Metadata;
use crate::oauth::PendingSignIn;
use crate::pricing::Liveness;
use crate::recharge::Recharge;
use crate::secret::Bearer;
use crate::secret::Secret;
use crate::state::{Ended, State};
use crate::store::SecretStore;

/// A poisoned lock is still usable: every guarded value stays consistent between statements.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// What the app holds for every connector: the config, the HTTP agent, the store, the state
/// file's contents and the liveness cache.
pub(crate) struct Shared {
    pub(crate) config: Config,
    pub(crate) agent: ureq::Agent,
    pub(crate) store: Arc<dyn SecretStore>,
    /// The state file as last read or written.
    pub(crate) state: Mutex<State>,
    pub(crate) liveness: Mutex<Option<Liveness>>,
    pub(crate) pricing_url: String,
    pub(crate) mcp_url: String,
    /// Milliseconds; the default whole-request timeout, which a test may shorten.
    request_timeout_ms: AtomicU64,
}

impl Shared {
    /// The whole-request timeout.
    pub(crate) fn timeout(&self) -> Duration {
        Duration::from_millis(self.request_timeout_ms.load(Ordering::Relaxed))
    }

    /// Unix seconds by the configured clock.
    pub(crate) fn unix_now(&self) -> u64 {
        unix(self.config.clock.now())
    }
}

/// `time` as unix seconds; 0 before 1970.
pub(crate) fn unix(time: SystemTime) -> u64 {
    time.duration_since(UNIX_EPOCH)
        .map_or(0, |age| age.as_secs())
}

/// The paid connectors, one per app. Build it once with the config and the keychain store.
pub struct Connectors {
    shared: Arc<Shared>,
    handles: Vec<ConnectorHandle>,
}

impl fmt::Debug for Connectors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Connectors")
            .field("config", &self.shared.config)
            .field("connectors", &self.handles)
            .finish_non_exhaustive()
    }
}

impl Connectors {
    /// Checks the origin, builds the HTTP agent and reads the state file. Makes no request and
    /// reads no keychain item.
    pub fn new(config: Config, store: Arc<dyn SecretStore>) -> Result<Connectors, Error> {
        config.origin.check()?;
        let agent = http::agent(&config)?;
        let state = State::load(&config.state_file)?;
        let case_law = config.origin.urls(&CASE_LAW);
        let shared = Arc::new(Shared {
            agent,
            store,
            state: Mutex::new(state),
            liveness: Mutex::new(None),
            pricing_url: case_law.pricing_url,
            mcp_url: case_law.mcp_url,
            request_timeout_ms: AtomicU64::new(millis(http::REQUEST_TIMEOUT)),
            config,
        });
        let handles = KNOWN
            .iter()
            .map(|known| ConnectorHandle::new(&shared, known))
            .collect();
        Ok(Connectors { shared, handles })
    }

    /// Whether the case-law slot shows, and its price. Blocking; never errors. Live only for a
    /// 200 with `status: "live"`. An answer that parsed is kept 24 hours, a failure 10 minutes;
    /// nothing is fetched in the background.
    pub fn liveness(&self) -> Liveness {
        crate::pricing::cached_liveness(&self.shared)
    }

    /// The handle of a known connector (`case-law`); `None` for any other id. Every call for
    /// the same id shares one credential.
    pub fn get(&self, id: &str) -> Option<ConnectorHandle> {
        self.handles
            .iter()
            .find(|handle| handle.inner.known.id == id)
            .cloned()
    }

    /// Shortens the whole-request timeout, so a test of a hung connector ends quickly.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn set_request_timeout_for_test(&self, timeout: Duration) {
        self.shared
            .request_timeout_ms
            .store(millis(timeout), Ordering::Relaxed);
    }
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

/// The credential of one connector, in memory.
pub(crate) enum Slot {
    /// The keychain item was not read yet.
    Unread,
    /// Read (or written) once; `None` when there is none.
    Read(Option<Credential>),
}

/// One connector's state: the credential slot, whose lock also makes the refresh single-flight,
/// the discovery metadata, the pending sign-in and the account cache.
pub(crate) struct HandleInner {
    pub(crate) shared: Arc<Shared>,
    pub(crate) known: &'static KnownConnector,
    pub(crate) urls: ConnectorUrls,
    pub(crate) slot: Mutex<Slot>,
    pub(crate) persist_failed: AtomicBool,
    /// Bumped whenever the credential is replaced or ends; the account cache is kept only for
    /// the generation it was read under.
    pub(crate) generation: AtomicU64,
    /// The last account read and the generation it belongs to.
    pub(crate) account: Mutex<Option<(u64, Account)>>,
    pub(crate) metadata: Mutex<Option<Arc<Metadata>>>,
    /// The pending sign-in's number and cancel flag.
    pub(crate) pending: Mutex<Option<(u64, CancelFlag)>>,
    pub(crate) next_sign_in: AtomicU64,
    #[cfg(feature = "test-support")]
    pub(crate) test_rng: Mutex<Option<rand::rngs::StdRng>>,
}

/// One known connector. Cheap to clone; every clone shares the credential. `Send + Sync`.
#[derive(Clone)]
pub struct ConnectorHandle {
    pub(crate) inner: Arc<HandleInner>,
}

impl fmt::Debug for ConnectorHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConnectorHandle")
            .field("id", &self.inner.known.id)
            .field(
                "persist_failed",
                &self.inner.persist_failed.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl ConnectorHandle {
    fn new(shared: &Arc<Shared>, known: &'static KnownConnector) -> ConnectorHandle {
        ConnectorHandle {
            inner: Arc::new(HandleInner {
                shared: Arc::clone(shared),
                known,
                urls: shared.config.origin.urls(known),
                slot: Mutex::new(Slot::Unread),
                persist_failed: AtomicBool::new(false),
                generation: AtomicU64::new(0),
                account: Mutex::new(None),
                metadata: Mutex::new(None),
                pending: Mutex::new(None),
                next_sign_in: AtomicU64::new(0),
                #[cfg(feature = "test-support")]
                test_rng: Mutex::new(None),
            }),
        }
    }

    /// The compiled-in connector.
    pub fn known(&self) -> &'static KnownConnector {
        self.inner.known
    }

    /// What the app shows. Reads the keychain item once, the first time the connector is added;
    /// after that, memory only.
    pub fn status(&self) -> CredentialStatus {
        let mut slot = lock(&self.inner.slot);
        if !self.is_added() {
            return CredentialStatus::NotAdded;
        }
        let kind = match self.load(&mut slot) {
            Ok(credential) => credential.map(Credential::kind),
            Err(err) => {
                tracing::warn!(connector = self.id(), error = %err, "the keychain item could not be read");
                None
            }
        };
        lock(&self.inner.shared.state).status(self.id(), kind)
    }

    /// Starts a sign-in: discovery with every check (before any listener opens), PKCE S256 and
    /// `state`, the loopback listener and the authorize URL. A pending sign-in of this connector
    /// is cancelled.
    pub fn begin_sign_in(&self, login_hint: Option<&str>) -> Result<PendingSignIn, SignInError> {
        crate::oauth::begin_sign_in(self, login_hint)
    }

    /// The bearer token: the API key, or an access token refreshed first when under 5 minutes
    /// of it remain. One refresh at a time; no I/O when none is due.
    pub fn bearer(&self) -> Result<Bearer, AuthError> {
        crate::refresh::bearer(self)
    }

    /// POSTs one MCP message; see [`McpReply`]. Sends `Authorization` only with a credential.
    pub fn mcp_post(&self, body: &[u8], headers: &McpRequestHeaders) -> Result<McpReply, Error> {
        crate::calls::mcp_post(self, body, headers)
    }

    /// The account; a copy younger than
    /// [`ACCOUNT_MIN_INTERVAL`](crate::ACCOUNT_MIN_INTERVAL) comes from memory.
    pub fn account(&self) -> Result<Account, Error> {
        crate::calls::account(self)
    }

    /// Opens a recharge for `pack`; its pay link is always ours.
    pub fn create_recharge(&self, pack: &str) -> Result<Recharge, Error> {
        crate::calls::create_recharge(self, pack)
    }

    /// Polls the recharge until it is paid (or otherwise settled), the recharge deadline
    /// passes, or `cancel` is set.
    pub fn wait_recharge(&self, recharge_id: &str, cancel: &CancelFlag) -> Result<Recharge, Error> {
        crate::calls::wait_recharge(self, recharge_id, cancel)
    }

    /// Seeds the generator of the verifier and `state`, for the authorize URL snapshot.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn seed_rng_for_test(&self, seed: u64) {
        crate::oauth::seed_rng_for_test(self, seed);
    }

    /// Whether a rotated refresh token is held only in memory, because the keychain refused it
    /// twice: the next app start will need a new sign-in.
    pub fn persist_failed(&self) -> bool {
        self.inner.persist_failed.load(Ordering::Relaxed)
    }

    /// Adds a firm's pasted API key: refuses anything that is not `cdk_live_` and 32 base62
    /// characters before any request, then checks it with one account read and stores it.
    pub fn add_api_key(&self, key: &str) -> Result<(), Error> {
        if !crate::credential::is_api_key(key) {
            return Err(Error::BadKeyFormat);
        }
        let key = Secret::from(key);
        let auth = Zeroizing::new(format!("Bearer {}", key.expose()));
        let url = self.account_url();
        let headers = [
            ("Authorization", auth.as_str()),
            ("Accept", "application/json"),
        ];
        let shared = &self.inner.shared;
        let reply = http::send(
            &shared.agent,
            "GET",
            &url,
            &headers,
            SendBody::None,
            shared.timeout(),
        )
        .map_err(Error::Network)?;
        match reply.status {
            // A 402 is a valid key on an empty account.
            200 | 402 => {}
            401 => return Err(Error::Auth(AuthError::KeyRevoked)),
            status => return Err(Error::Http { status }),
        }
        let credential = Credential::ApiKey(key);
        self.store_new(credential, CredentialKind::ApiKey)?;
        if reply.status == 200 {
            let now = shared.config.clock.now();
            let parsed = crate::account::parse_account(
                &reply.body,
                self.inner.known,
                &shared.config.origin,
                now,
            );
            if let Ok(account) = parsed {
                let generation = self.inner.generation.load(Ordering::SeqCst);
                *lock(&self.inner.account) = Some((generation, account));
            }
        }
        Ok(())
    }

    /// Signs out: revokes the refresh token (`POST /revoke`; an API key has no revoke call),
    /// then deletes the keychain item, then clears the credential in memory. The item is
    /// deleted whether or not the revoke worked.
    pub fn sign_out(&self) -> Result<SignOutReport, Error> {
        if let Some((_, pending)) = lock(&self.inner.pending).take() {
            pending.cancel();
        }
        let mut slot = lock(&self.inner.slot);
        let credential = match self.load(&mut slot) {
            Ok(credential) => credential.cloned(),
            Err(err) => {
                tracing::warn!(connector = self.id(), error = %err, "the keychain item could not be read; deleting it");
                None
            }
        };
        let revoked = match &credential {
            Some(Credential::OAuth(tokens)) => crate::refresh::revoke(self, &tokens.refresh_token),
            Some(Credential::ApiKey(_)) | None => false,
        };
        let deleted = self.inner.shared.store.delete(self.id());
        *slot = Slot::Read(None);
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        self.inner.persist_failed.store(false, Ordering::Relaxed);
        self.save_state(|state| state.end(self.id(), Ended::SignedOut))?;
        deleted?;
        Ok(SignOutReport { revoked })
    }

    /// Signs out, then forgets the connector on this install.
    pub fn remove(&self) -> Result<(), Error> {
        let signed_out = self.sign_out();
        // The entry goes even when the keychain refused the delete.
        let _guard = lock(&self.inner.slot);
        self.save_state(|state| state.remove(self.id()))?;
        signed_out.map(|_| ())
    }

    pub(crate) fn id(&self) -> &'static str {
        self.inner.known.id
    }

    pub(crate) fn account_url(&self) -> String {
        format!("{}/account?recent_days=30", self.inner.urls.api_base)
    }

    fn is_added(&self) -> bool {
        lock(&self.inner.shared.state)
            .connectors
            .contains_key(self.id())
    }

    /// The credential, reading the keychain item the first time. An item that does not parse
    /// is deleted and the connector marked signed out.
    pub(crate) fn load<'a>(
        &self,
        slot: &'a mut Slot,
    ) -> Result<Option<&'a Credential>, StoreError> {
        if matches!(slot, Slot::Unread) {
            let read = match self.inner.shared.store.get(self.id())? {
                None => None,
                Some(bytes) => {
                    let bytes = Zeroizing::new(bytes);
                    let decoded = Credential::decode(&bytes);
                    if decoded.is_none() {
                        tracing::warn!(
                            connector = self.id(),
                            "the keychain item does not parse; deleted"
                        );
                        if let Err(err) = self.inner.shared.store.delete(self.id()) {
                            tracing::warn!(connector = self.id(), error = %err, "the keychain item could not be deleted");
                        }
                        self.mark_ended(Ended::SignedOut);
                    }
                    decoded
                }
            };
            *slot = Slot::Read(read);
        }
        Ok(match slot {
            Slot::Read(credential) => credential.as_ref(),
            Slot::Unread => None,
        })
    }

    /// The usable credential, or why there is none.
    pub(crate) fn current<'a>(&self, slot: &'a mut Slot) -> Result<&'a Credential, AuthError> {
        if !self.is_added() {
            return Err(AuthError::NotAdded);
        }
        if let Some(credential) = self.load(slot)? {
            return Ok(credential);
        }
        match lock(&self.inner.shared.state).status(self.id(), None) {
            CredentialStatus::KeyRevoked => Err(AuthError::KeyRevoked),
            CredentialStatus::NotAdded => Err(AuthError::NotAdded),
            CredentialStatus::SignedOut | CredentialStatus::SignedIn(_) => {
                Err(AuthError::SignedOut)
            }
        }
    }

    /// Ends the credential: deletes the item, clears memory and records `ended`.
    pub(crate) fn end_credential(&self, slot: &mut Slot, ended: Ended) {
        if let Err(err) = self.inner.shared.store.delete(self.id()) {
            tracing::warn!(connector = self.id(), error = %err, "the keychain item could not be deleted");
        }
        *slot = Slot::Read(None);
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        self.mark_ended(ended);
    }

    fn mark_ended(&self, ended: Ended) {
        if let Err(err) = self.save_state(|state| state.end(self.id(), ended)) {
            tracing::warn!(connector = self.id(), error = %err, "the state file could not be written");
        }
    }

    /// Changes the state and writes the file; memory follows only when the write worked.
    pub(crate) fn save_state(&self, change: impl FnOnce(&mut State)) -> Result<(), StoreError> {
        let shared = &self.inner.shared;
        let mut state = lock(&shared.state);
        let mut next = state.clone();
        change(&mut next);
        if next == *state {
            return Ok(());
        }
        next.save(&shared.config.state_file)?;
        *state = next;
        Ok(())
    }

    /// Stores a new credential: the keychain first, then the state file (added, `ended: null`),
    /// then memory. A keychain failure leaves the state file as it was.
    pub(crate) fn store_new(
        &self,
        credential: Credential,
        kind: CredentialKind,
    ) -> Result<(), StoreError> {
        let bytes = credential.encode()?;
        let mut slot = lock(&self.inner.slot);
        self.inner.shared.store.set(self.id(), &bytes)?;
        self.save_state(|state| state.add(self.id(), kind))?;
        *slot = Slot::Read(Some(credential));
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        self.inner.persist_failed.store(false, Ordering::Relaxed);
        Ok(())
    }

    /// The `Authorization` value for `token`.
    pub(crate) fn bearer_header(token: &Secret) -> Zeroizing<String> {
        let mut value = Zeroizing::new(String::with_capacity(token.len().saturating_add(7)));
        value.push_str("Bearer ");
        value.push_str(token.expose());
        value
    }
}
