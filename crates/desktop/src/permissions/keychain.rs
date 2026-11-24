//! The `keychain` row and its Ask. The Ask reads each would-ask item one by one with keychain
//! dialogs on (`quiet.rs`), off again right after the last; a granted Ask then has the agents'
//! keys read again ([`KeychainGranted`]), so a key the start's quiet read could not take
//! reaches its agent without a restart.

use std::sync::Arc;

use crate::setup::KeySlot;

use super::store::{Answer, Store};
use super::{KeychainCheck, KeychainItem, OsPermissionState, PermissionId, PermissionRow, Probe};
use super::{keep, quiet};

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
fn after_keychain_granted(keys: &KeySlot) {
    if let Some(cache) = keys.get() {
        cache.read_again_all();
    }
}

/// Asks for `items` through `probe`, records the answer in `store` and answers the row; runs
/// `granted` once when every item read.
pub(super) fn ask_keychain(
    probe: &dyn Probe,
    store: &Store,
    items: Vec<KeychainItem>,
    granted: &dyn Fn(),
) -> PermissionRow {
    // One by one: macOS shows one dialog per item. Dialogs on for these reads only.
    let refused: Vec<KeychainItem> = quiet::with_dialogs_allowed(|| {
        items
            .into_iter()
            .filter(|item| !probe.read_keychain(item))
            .collect()
    });
    let answer = Answer::from_granted(refused.is_empty());
    tracing::info!(
        ?answer,
        refused = refused.len(),
        "permissions: asked the keychain"
    );
    keep(store, PermissionId::Keychain, answer);
    if refused.is_empty() {
        granted();
    }
    PermissionRow {
        id: PermissionId::Keychain,
        state: answer.state(),
        folder: None,
        settings_path: None,
        note: items_note(&refused),
    }
}

/// `keychain`'s row, following what the silent check finds now at every listing, with the
/// answer recorded on this build (`recorded`) only where the check cannot say more:
///
/// - an item would ask: `would_ask`, or `denied` when the recorded answer is a refusal (still
///   refused while a read would ask). A recorded grant never hides an item that would ask (an
///   "Allow" once, not "Always Allow", leaves the next read asking again).
/// - none would ask, one could not be told: the recorded answer, else `unknown`.
/// - none would ask or is unknown: `granted` when Clauseo asked on this build (whatever was
///   answered then: every item reads now), so the row stays shown and is not asked again;
///   otherwise not listed.
pub(super) fn keychain_row(
    check: &KeychainCheck,
    recorded: Option<Answer>,
) -> Option<PermissionRow> {
    let state = if !check.would_ask.is_empty() {
        match recorded {
            Some(Answer::Denied) => OsPermissionState::Denied,
            Some(Answer::Granted) | None => OsPermissionState::WouldAsk,
        }
    } else if !check.unknown.is_empty() {
        recorded.map_or(OsPermissionState::Unknown, Answer::state)
    } else {
        recorded?;
        OsPermissionState::Granted
    };
    let items: Vec<KeychainItem> = check
        .would_ask
        .iter()
        .chain(&check.unknown)
        .cloned()
        .collect();
    Some(PermissionRow {
        id: PermissionId::Keychain,
        state,
        folder: None,
        settings_path: None,
        note: items_note(&items),
    })
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
