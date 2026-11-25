//! The `keychain` row and its Ask. The Ask reads each would-ask item one by one in a child
//! process (`ask_child.rs`), the only place macOS's dialog can show (this process keeps
//! dialogs off, `quiet.rs`); then the silent check runs again here and the row follows what it
//! finds. A granted Ask has the agents' keys read again ([`KeychainGranted`]), so a key the
//! start's quiet read could not take reaches its agent without a restart.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_protocol::HostError;

use crate::setup::KeySlot;

use super::ask_child::{AskResult, Outcome};
use super::keep;
use super::logged::LastLogged;
use super::store::{Answer, Store};
use super::{KeychainCheck, KeychainItem, OsPermissionState, PermissionId, PermissionRow, Probe};

/// What a granted keychain Ask runs: setup's read again of the agents' keys that did not read
/// (`KeyCache::read_again_all`). Managed by the app; cheap to clone.
#[derive(Clone)]
pub(crate) struct KeychainGranted(Arc<dyn Fn() + Send + Sync>);

impl KeychainGranted {
    /// The app's: [`after_keychain_granted`] over `keys`.
    pub(crate) fn reading(keys: KeySlot) -> Self {
        Self::new(move || after_keychain_granted(&keys))
    }

    fn new(run: impl Fn() + Send + Sync + 'static) -> Self {
        Self(Arc::new(run))
    }

    /// Runs it, marking `fired` first, so the caller then has the host read again
    /// ([`host_read_again`]).
    pub(crate) fn run_marking(&self, fired: &AtomicBool) {
        fired.store(true, Ordering::SeqCst);
        self.run();
    }

    /// Runs it.
    pub(crate) fn run(&self) {
        (self.0)();
    }
}

/// What follows a granted keychain Ask, so what start's quiet reads could not take arrives
/// without a restart: each agent key whose read failed is read again (`KeyCache::read_again`;
/// its answer refreshes the agent's row through `KeyNote::Read`), once the setup thread has
/// loaded the keys into `keys` (before then, its own load reads them all). MCP secrets need
/// nothing: they are read at each launch.
///
/// The host's own reads (the case-law credential, the blocked-site allow-list hash) are read
/// again by a host command still to come: its one call goes here.
/// The host's part of a granted Ask, once [`after_keychain_granted`] has run (`fired`): its
/// `KeychainReadAgain` reads again what the keychain refused at its start (the case-law
/// credential, saved tool secrets, the allow-list's hash) and sends its changed events. A
/// failure is logged on the lifecycle target; the row stands.
pub(super) async fn host_read_again(fired: bool, bridge: &crate::bridge::Bridge) {
    if fired && let Err(error) = bridge.keychain_read_again().await {
        tracing::warn!(
            target: "clauseo_desktop::permissions::lifecycle",
            code = ?error.code(),
            "permissions: the host's keychain read-again failed"
        );
    }
}

/// The desktop's part: the agents' keys read again. The host's part (`KeychainReadAgain`, the
/// case-law credential, saved tool secrets, the allow-list's hash) is sent by `ask_permission`
/// once this has run (`Bridge::keychain_read_again`).
fn after_keychain_granted(keys: &KeySlot) {
    if let Some(cache) = keys.get() {
        cache.read_again_all();
    }
}

/// What the Ask came to, from the child's answers and the silent check run again after it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Decision {
    /// Nothing would ask any more ("Always Allow"): recorded granted, the keys read again.
    Granted,
    /// The child read every item that still would ask: the lawyer chose "Allow" once. Recorded
    /// as granted (asked on this build), so the row says `would_ask` with `asked`: "choose
    /// Always Allow".
    AllowedOnce,
    /// An item that still would ask was refused (Deny, a cancel) or never answered.
    Denied,
}

impl Decision {
    /// The answer kept in the store.
    fn answer(self) -> Answer {
        match self {
            Self::Granted | Self::AllowedOnce => Answer::Granted,
            Self::Denied => Answer::Denied,
        }
    }
}

/// What the child's `results` and the check run after it (`recheck`) come to: the fact wins.
pub(super) fn decide(results: &[AskResult], recheck: &KeychainCheck) -> Decision {
    if recheck.would_ask.is_empty() {
        return Decision::Granted;
    }
    let read_by_child = |item: &KeychainItem| {
        results
            .iter()
            .any(|result| result.is_for(item) && result.outcome == Outcome::Read)
    };
    if recheck.would_ask.iter().all(read_by_child) {
        Decision::AllowedOnce
    } else {
        Decision::Denied
    }
}

/// Asks for `items` through `probe`'s child process, checks again, records the answer in
/// `store` and answers the row; runs `granted` once when nothing would ask any more. While
/// the child runs, a listing or a second Ask answers `asking` at once. No value passes from
/// the child to anything: its answers name items and outcomes only.
pub(super) fn ask_keychain(
    probe: &dyn Probe,
    store: &Store,
    items: &[KeychainItem],
    granted: &dyn Fn(),
) -> Result<PermissionRow, HostError> {
    let asking = probe.keychain_asking();
    if asking
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        let recorded = store.asked().contains_key(&PermissionId::Keychain);
        return Ok(asking_row(recorded));
    }
    let _done = AskDone(asking);
    let results = probe.ask_keychain(items)?;
    let recheck = probe.keychain();
    let decision = decide(&results, &recheck);
    let answer = decision.answer();
    tracing::info!(
        ?decision,
        items = items.len(),
        still_would_ask = recheck.would_ask.len(),
        "permissions: asked the keychain"
    );
    keep(store, PermissionId::Keychain, answer);
    if decision == Decision::Granted {
        granted();
    }
    let state = keychain_state(&recheck, Some(answer)).unwrap_or(OsPermissionState::Granted);
    Ok(row(state, &recheck, true))
}

/// Ends the Ask's `asking` mark when dropped, however the Ask ends.
struct AskDone<'a>(&'a AtomicBool);

impl Drop for AskDone<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The keychain row while the Ask's child runs; `asked` from the store.
pub(super) fn asking_row(asked: bool) -> PermissionRow {
    row(OsPermissionState::Asking, &KeychainCheck::default(), asked)
}

/// What the row's lifecycle line said last: state, would-ask and unknown counts, recorded answer.
type RowLine = (Option<OsPermissionState>, usize, usize, Option<Answer>);
static ROW_LOGGED: LastLogged<(), RowLine> = LastLogged::new();

/// `keychain`'s row, following what the silent check finds now at every listing, with the
/// answer recorded on this build (`recorded`) only where the check cannot say more
/// ([`keychain_state`]); `asked` when one is recorded.
pub(super) fn keychain_row(
    check: &KeychainCheck,
    recorded: Option<Answer>,
) -> Option<PermissionRow> {
    let state = keychain_state(check, recorded);
    // On the lifecycle target (host.log keeps it), and only when it changed since the last line
    // (the app lists at every focus): what the row says and how many items it rests on, never an
    // account or a value.
    let (would_ask, unknown) = (check.would_ask.len(), check.unknown.len());
    if ROW_LOGGED.changed((), (state, would_ask, unknown, recorded)) {
        tracing::info!(
            target: "clauseo_desktop::permissions::lifecycle",
            ?state,
            would_ask,
            unknown,
            recorded = ?recorded,
            "permissions: keychain row"
        );
    }
    state.map(|state| row(state, check, recorded.is_some()))
}

/// The keychain row's state:
///
/// - an item would ask: `would_ask`, or `denied` when the recorded answer is a refusal (still
///   refused while a read would ask). A recorded grant never hides an item that would ask (an
///   "Allow" once, not "Always Allow", leaves the next read asking again).
/// - none would ask, one could not be told: `unknown`, or `denied` after a refusal; a recorded
///   grant never shows over an item that could not be told.
/// - none would ask or is unknown: `granted` when Clauseo asked on this build (whatever was
///   answered then: every item reads now), so the row stays shown and is not asked again;
///   otherwise not listed.
pub(super) fn keychain_state(
    check: &KeychainCheck,
    recorded: Option<Answer>,
) -> Option<OsPermissionState> {
    let refused = recorded == Some(Answer::Denied);
    Some(if !check.would_ask.is_empty() {
        if refused {
            OsPermissionState::Denied
        } else {
            OsPermissionState::WouldAsk
        }
    } else if !check.unknown.is_empty() {
        if refused {
            OsPermissionState::Denied
        } else {
            OsPermissionState::Unknown
        }
    } else {
        recorded?;
        OsPermissionState::Granted
    })
}

/// The keychain row in `state`, naming `check`'s items.
fn row(state: OsPermissionState, check: &KeychainCheck, asked: bool) -> PermissionRow {
    let items: Vec<KeychainItem> = check
        .would_ask
        .iter()
        .chain(&check.unknown)
        .cloned()
        .collect();
    PermissionRow {
        id: PermissionId::Keychain,
        state,
        folder: None,
        settings_path: None,
        note: items_note(&items),
        asked,
    }
}

/// The items by service and account, never a value; `None` for none.
fn items_note(items: &[KeychainItem]) -> Option<String> {
    (!items.is_empty()).then(|| {
        items
            .iter()
            .map(|item| format!("{} ({})", item.service, item.account))
            .collect::<Vec<_>>()
            .join(", ")
    })
}
