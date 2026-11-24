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
use crate::config::{Config, ConnectorUrls, Origin};
use crate::credential::{
    Credential, CredentialKind, CredentialStatus, RemoveReport, SignOutReport,
};
use crate::error::{AuthError, Error, SignInError, StoreError};
use crate::http;
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
    pub(crate) agent: http::Agents,
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
        if !config.origin.allowed() {
            return Err(Error::BadOrigin);
        }
        let agent = http::Agents::new(&config)?;
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

    /// [`Self::liveness`], except that a cached answer that is not live is fetched again: the
    /// host's re-check when the lawyer opens the skills and tools list (the host paces it).
    pub fn recheck_liveness(&self) -> Liveness {
        crate::pricing::liveness_with(&self.shared, true)
    }

    /// The origin the config named: where every request goes and whose pay links are taken.
    /// Fixed for the life of the value; the host reads it to recheck a pay link before it opens
    /// one (`PayUrl::is_for`).
    pub fn origin(&self) -> &Origin {
        &self.shared.config.origin
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
    /// Whether the last read of the keychain item was refused (an error, not "no item"): no
    /// automatic delete runs while it is, since what the item holds is not known. A read that
    /// works, or a write that lands, clears it.
    pub(crate) read_refused: AtomicBool,
    /// Bumped whenever the credential is replaced or ends; the account cache is kept only for
    /// the generation it was read under.
    pub(crate) generation: AtomicU64,
    /// Bumped by [`ConnectorHandle::forget_account`]: the account cache is kept only for the
    /// count it was read under, so a read in flight when it is forgotten does not refill it.
    pub(crate) forgets: AtomicU64,
    /// The last account read, with the generation and the forget count it belongs to.
    pub(crate) account: Mutex<Option<(u64, u64, Account)>>,
    pub(crate) metadata: Mutex<Option<Arc<Metadata>>>,
    /// The pending sign-in's number and cancel flag.
    pub(crate) pending: Mutex<Option<(u64, CancelFlag)>>,
    pub(crate) next_sign_in: AtomicU64,
    #[cfg(feature = "test-support")]
    pub(crate) test_rng: Mutex<Option<rand::rngs::StdRng>>,
    /// How many loopback listeners sign-in asked `bind_loopback_exclusive` for.
    #[cfg(feature = "test-support")]
    pub(crate) listeners_bound: std::sync::atomic::AtomicUsize,
    /// The load of the latest sign-in's listener.
    pub(crate) listener_stats: Mutex<Option<Arc<crate::oauth::ListenerStats>>>,
}

/// The latest sign-in listener's load: reader threads and queued connections, now and at the
/// peak (test-support only).
#[cfg(feature = "test-support")]
#[doc(hidden)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListenerLoad {
    /// Reader threads now.
    pub readers: usize,
    /// Connections queued for a reader now.
    pub queued: usize,
    /// The most reader threads at once.
    pub peak_readers: usize,
    /// The longest the queue was.
    pub peak_queued: usize,
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
                read_refused: AtomicBool::new(false),
                generation: AtomicU64::new(0),
                forgets: AtomicU64::new(0),
                account: Mutex::new(None),
                metadata: Mutex::new(None),
                pending: Mutex::new(None),
                next_sign_in: AtomicU64::new(0),
                #[cfg(feature = "test-support")]
                test_rng: Mutex::new(None),
                #[cfg(feature = "test-support")]
                listeners_bound: std::sync::atomic::AtomicUsize::new(0),
                listener_stats: Mutex::new(None),
            }),
        }
    }

    /// The compiled-in connector.
    pub fn known(&self) -> &'static KnownConnector {
        self.inner.known
    }

    /// What the app shows. Reads the keychain item once, the first time the connector is added;
    /// after that, memory only. A read the keychain refused (any error but "no item") is
    /// `Unreadable`, never signed out, and the next call reads the item again.
    pub fn status(&self) -> CredentialStatus {
        let mut slot = lock(&self.inner.slot);
        if !self.is_added() {
            return CredentialStatus::NotAdded;
        }
        let kind = match self.load(&mut slot) {
            Ok(credential) => credential.map(Credential::kind),
            Err(err) => {
                tracing::warn!(connector = self.id(), error = %err, "the keychain item could not be read");
                return CredentialStatus::Unreadable;
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

    /// Drops the account's copy, so the next [`ConnectorHandle::account`] asks the connector:
    /// for a balance that changed where this crate did not see it (a paid `recharge` tool
    /// result through the relay).
    /// Never waits: a read in flight keeps the lock, and stores nothing once this is called.
    pub fn forget_account(&self) {
        self.inner.forgets.fetch_add(1, Ordering::SeqCst);
    }

    /// Opens a recharge for `pack`; its pay link is always ours.
    pub fn create_recharge(&self, pack: &str) -> Result<Recharge, Error> {
        crate::calls::create_recharge(self, pack)
    }

    /// Polls the recharge (a plain GET every
    /// [`Config::recharge_interval`](crate::Config::recharge_interval), never `wait` or
    /// `resume`) until it is settled, the recharge deadline passes, or `cancel` is set.
    pub fn wait_recharge(&self, recharge_id: &str, cancel: &CancelFlag) -> Result<Recharge, Error> {
        crate::calls::wait_recharge(self, recharge_id, cancel)
    }

    /// Seeds the generator of the verifier and `state`, for the authorize URL snapshot.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn seed_rng_for_test(&self, seed: u64) {
        crate::oauth::seed_rng_for_test(self, seed);
    }

    /// How many times sign-in called `bind_loopback_exclusive` on this connector.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn listeners_bound_for_test(&self) -> usize {
        self.inner.listeners_bound.load(Ordering::SeqCst)
    }

    /// The latest sign-in listener's load; `None` before any sign-in.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    pub fn listener_stats_for_test(&self) -> Option<ListenerLoad> {
        lock(&self.inner.listener_stats)
            .as_ref()
            .map(|stats| ListenerLoad {
                readers: stats.readers.load(Ordering::SeqCst),
                queued: stats.queued.load(Ordering::SeqCst),
                peak_readers: stats.peak_readers.load(Ordering::SeqCst),
                peak_queued: stats.peak_queued.load(Ordering::SeqCst),
            })
    }

    /// Whether a rotated refresh token is held only in memory, because the keychain refused it
    /// twice: the next app start will need a new sign-in.
    pub fn persist_failed(&self) -> bool {
        self.inner.persist_failed.load(Ordering::Relaxed)
    }

    /// Adds a firm's pasted API key: refuses anything that is not `cdk_live_` and 32 base62
    /// characters before any request, then checks it with one account read and stores it.
    pub fn add_api_key(&self, key: &str) -> Result<(), Error> {
        self.add_api_key_while(key, &|| true)
    }

    /// As [`add_api_key`](Self::add_api_key), storing the key only if `still` answers `true`
    /// when asked under the credential's lock, just before the store: a caller whose sign-out
    /// or switch-off came in while the key was checked gets `Error::Cancelled`, and nothing is
    /// stored.
    pub fn add_api_key_while(&self, key: &str, still: &dyn Fn() -> bool) -> Result<(), Error> {
        crate::calls::add_api_key(self, key, still)
    }

    /// Signs out: revokes the refresh token (`POST /revoke`; an API key has no revoke call),
    /// then deletes the keychain item, then clears the credential in memory. The item is
    /// deleted whether or not the revoke worked.
    ///
    /// On macOS ([`KeychainOs::refuses_unreadable`](crate::KeychainOs::refuses_unreadable)) a
    /// keychain that refuses to read the item (an error that is not "no item") refuses the
    /// sign-out with `StoreError::Refused`, before anything is revoked, deleted or recorded:
    /// its refresh token could not be revoked. Elsewhere what can't be read is not revoked and
    /// the delete goes ahead. A delete the keychain refuses records nothing and keeps the item:
    /// `Refused` when no revoke was asked for (an API key), else the keychain's own error, since
    /// the revoke may have ended the sign-in.
    pub fn sign_out(&self) -> Result<SignOutReport, Error> {
        crate::sign_out::sign_out(self)
    }

    /// Signs out, then forgets the connector on this install. A sign-out that left the item in
    /// the keychain (its read refused on macOS, so nothing was revoked or deleted; or its delete
    /// refused) still forgets the connector here, and says so: `left_in_keychain`. A later Add
    /// reads that item again.
    pub fn remove(&self) -> Result<RemoveReport, Error> {
        crate::sign_out::remove(self)
    }

    pub(crate) fn id(&self) -> &'static str {
        self.inner.known.id
    }

    /// `StoreError::Refused` for a keychain call that failed, logged without the item.
    pub(crate) fn unreadable(&self, err: &StoreError) -> StoreError {
        tracing::warn!(connector = self.id(), error = %err, "the keychain refused the item; nothing was changed");
        match err {
            StoreError::Refused(_) => err.clone(),
            other => StoreError::Refused(other.to_string()),
        }
    }

    /// On macOS, reads the item now if it was not read yet (memory only after that), so a
    /// sign-in or a new key is refused with `StoreError::Refused` before it starts while the
    /// keychain refuses the read: the credential it would replace could not be revoked.
    /// Elsewhere nothing is read here (`KeychainOs::refuses_unreadable`).
    pub(crate) fn check_readable(&self) -> Result<(), StoreError> {
        if !self.refuses_unreadable() {
            return Ok(());
        }
        let mut slot = lock(&self.inner.slot);
        self.load(&mut slot)
            .map(drop)
            .map_err(|err| self.unreadable(&err))
    }

    /// Whether the lawyer's own commands are refused while the item can't be read: on macOS.
    pub(crate) fn refuses_unreadable(&self) -> bool {
        self.inner.shared.config.keychain_os.refuses_unreadable()
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
    /// is deleted and the connector marked signed out; a delete the keychain refuses changes
    /// nothing and is `StoreError::Refused`.
    pub(crate) fn load<'a>(
        &self,
        slot: &'a mut Slot,
    ) -> Result<Option<&'a Credential>, StoreError> {
        if matches!(slot, Slot::Unread) {
            let read = match self.get_item()? {
                None => None,
                Some(bytes) => {
                    let bytes = Zeroizing::new(bytes);
                    let decoded = Credential::decode(&bytes);
                    if decoded.is_none() {
                        tracing::warn!(
                            connector = self.id(),
                            "the keychain item does not parse; deleted"
                        );
                        // A delete refused leaves the item and the state file, and the slot
                        // unread: the next look reads it again.
                        self.inner
                            .shared
                            .store
                            .delete(self.id())
                            .map_err(|err| self.unreadable(&err))?;
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

    /// The keychain item read again now, past the slot: `None` when it is gone or does not
    /// parse.
    pub(crate) fn read_stored(&self) -> Result<Option<Credential>, StoreError> {
        Ok(self
            .get_item()?
            .and_then(|bytes| Credential::decode(&Zeroizing::new(bytes))))
    }

    /// The keychain item's bytes, noting whether the read was refused (`read_refused`).
    fn get_item(&self) -> Result<Option<Vec<u8>>, StoreError> {
        let read = self.inner.shared.store.get(self.id());
        self.inner
            .read_refused
            .store(read.is_err(), Ordering::SeqCst);
        read
    }

    /// Notes that a write landed: the item holds what this handle wrote.
    pub(crate) fn wrote_item(&self) {
        self.inner.read_refused.store(false, Ordering::SeqCst);
    }

    /// The usable credential, or why there is none.
    pub(crate) fn current<'a>(&self, slot: &'a mut Slot) -> Result<&'a Credential, AuthError> {
        if !self.is_added() {
            return Err(AuthError::NotAdded);
        }
        // Any keychain error here is a refusal, never "no item": `Refused`, as every caller
        // tells it apart from a sign-in that ended.
        if let Some(credential) = self
            .load(slot)
            .map_err(|err| AuthError::Store(self.unreadable(&err)))?
        {
            return Ok(credential);
        }
        match lock(&self.inner.shared.state).status(self.id(), None) {
            CredentialStatus::KeyRevoked => Err(AuthError::KeyRevoked),
            CredentialStatus::NotAdded => Err(AuthError::NotAdded),
            CredentialStatus::SignedOut
            | CredentialStatus::SignedIn(_)
            | CredentialStatus::Unreadable => Err(AuthError::SignedOut),
        }
    }

    /// Ends the credential: deletes the item, clears memory and records `ended`. Never while the
    /// last read of the item was refused (`read_refused`), and a delete the keychain refuses
    /// does nothing either: the item and the state file stay as they were and the slot unread
    /// (the next look reads the item again, `Unreadable` while refused): `StoreError::Refused`.
    pub(crate) fn end_credential(&self, slot: &mut Slot, ended: Ended) -> Result<(), StoreError> {
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
        if self.inner.read_refused.load(Ordering::SeqCst) {
            // The last read of the item was refused: it may hold another credential than the one
            // that ended, so nothing deletes it or records the end.
            *slot = Slot::Unread;
            return Err(self.unreadable(&StoreError::Refused(
                "the last read of the keychain item was refused".to_owned(),
            )));
        }
        if let Err(err) = self.inner.shared.store.delete(self.id()) {
            *slot = Slot::Unread;
            return Err(self.unreadable(&err));
        }
        *slot = Slot::Read(None);
        self.mark_ended(ended);
        Ok(())
    }

    /// Forgets the credential in memory without touching the item or the state file, so the
    /// next look reads the item again: after a keychain call refused.
    pub(crate) fn forget_unread(&self, slot: &mut Slot) {
        *slot = Slot::Unread;
        self.inner.generation.fetch_add(1, Ordering::SeqCst);
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
    /// then memory, all under the slot's lock. A keychain failure leaves the state file and the
    /// old credential as they were. Once the lock is released, the OAuth refresh tokens the new
    /// credential replaced (in memory or in the keychain) are revoked, best effort: a failure is
    /// logged without the token. The new credential's own token is never revoked.
    ///
    /// `still` is asked under the slot's lock first: `false` stores nothing and answers
    /// `Ok(false)` (the caller's credential ended while it was being made). On macOS a keychain
    /// that refuses to read the credential being replaced stores nothing and answers
    /// `StoreError::Refused`: its refresh token could not be revoked. A write it refuses
    /// answers the same, the old item and the state file as they were.
    pub(crate) fn store_new(
        &self,
        credential: Credential,
        kind: CredentialKind,
        still: &dyn Fn() -> bool,
    ) -> Result<bool, StoreError> {
        let bytes = credential.encode()?;
        let replaced = {
            let mut slot = lock(&self.inner.slot);
            if !still() {
                return Ok(false);
            }
            let replaced = self.replaced_refresh_tokens(&mut slot, &credential)?;
            if let Err(err) = self.inner.shared.store.set(self.id(), &bytes) {
                // The old item stays, as does the state file; the next look reads it again
                // (unless memory holds a newer token than the item).
                if !self.persist_failed() {
                    self.forget_unread(&mut slot);
                }
                return Err(self.unreadable(&err));
            }
            self.wrote_item();
            self.save_state(|state| state.add(self.id(), kind))?;
            *slot = Slot::Read(Some(credential));
            self.inner.generation.fetch_add(1, Ordering::SeqCst);
            self.inner.persist_failed.store(false, Ordering::Relaxed);
            replaced
        };
        for token in &replaced {
            if !crate::refresh::revoke(self, token) {
                tracing::warn!(
                    connector = self.id(),
                    "the replaced refresh token could not be revoked"
                );
            }
        }
        Ok(true)
    }

    /// The OAuth refresh tokens `new` replaces: the one in memory and the one in the keychain,
    /// without duplicates and never `new`'s own. The slot is locked by the caller. On macOS
    /// either read refused (an error that is not "no item") is `StoreError::Refused`.
    fn replaced_refresh_tokens(
        &self,
        slot: &mut Slot,
        new: &Credential,
    ) -> Result<Vec<Secret>, StoreError> {
        let own = match new {
            Credential::OAuth(tokens) => Some(&tokens.refresh_token),
            Credential::ApiKey(_) => None,
        };
        let mut old: Vec<Secret> = Vec::new();
        let mut keep = |token: Secret| {
            if own != Some(&token) && !old.contains(&token) {
                old.push(token);
            }
        };
        // On macOS a refused read stores nothing (`Refused`); elsewhere the write goes ahead
        // and what could not be read is not revoked.
        let refuses = self.refuses_unreadable();
        match self.load(slot) {
            Ok(Some(Credential::OAuth(tokens))) => keep(tokens.refresh_token.clone()),
            Ok(_) => {}
            Err(err) if refuses => return Err(self.unreadable(&err)),
            Err(err) => {
                tracing::warn!(connector = self.id(), error = %err, "the replaced credential could not be read");
            }
        }
        match self.read_stored() {
            Ok(Some(Credential::OAuth(tokens))) => keep(tokens.refresh_token),
            Ok(_) => {}
            Err(err) if refuses => return Err(self.unreadable(&err)),
            Err(err) => {
                tracing::warn!(connector = self.id(), error = %err, "the replaced keychain item could not be read");
            }
        }
        Ok(old)
    }

    /// The `Authorization` value for `token`.
    pub(crate) fn bearer_header(token: &Secret) -> Zeroizing<String> {
        let mut value = Zeroizing::new(String::with_capacity(token.len().saturating_add(7)));
        value.push_str("Bearer ");
        value.push_str(token.expose());
        value
    }
}
