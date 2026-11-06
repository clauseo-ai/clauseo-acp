//! The limits readers desktop gives the host (`HostConfig::with_limits_reader`), so the setup
//! backend can hold an agent's reads across a login change and swap in each entry it writes
//! (decision record 0048, the plan's ruling 7):
//!
//! - built over the host's launcher, with Clauseo's own homes forced (`CODEX_HOME` and, through
//!   `ProcessReaders::with_claude_home`, `CLAUDE_CONFIG_DIR`, ruling 6), and at each read the
//!   variables the launch hook adds (`with_launch_env` over `hook::setup_vars`): the stored key
//!   from the [`KeySlot`] and the firm plan's routing from the [`Plans`] snapshot (the security
//!   review's M4), so a read goes where the agent's launches go;
//! - [`SwappableReaders::set_entry`] sets the entry with `ProcessReaders::set_entry` after each
//!   `EntryWrite::Written`; a read takes its entry once past its gate, so a read queued behind
//!   a hold reads the entry written before the hold drops (the review's L7);
//! - [`ReadHold`] is the hold the backend keeps across the leave order, as [`LeaveReaders`]
//!   names it, so the backend's tests play it with a fake.
//! - No read goes out before the keys have loaded (R20 P5b A4, c1): a read with no key could
//!   read another login's numbers (a stale own login in the home, or the reused one) for an
//!   agent whose key is stored. A read waits for the key slot, at most [`KEYS_HOLD`], checked
//!   every [`KEYS_POLL`] on the readers' timer, and fails if it stays empty; an agent whose
//!   stored key is unreadable fails its read too. A failed read shows no account's numbers.
//! - No read goes out for an agent an unreadable policy blocks (`signin.policy_unreadable`,
//!   review M2): with no plan to read, its read would take a stored key, perhaps the firm
//!   gateway's, to the vendor's own host while its launches are refused. The blocked agents
//!   come from the settings read at start, then from each `SettingsChanged`
//!   ([`LeaveReaders::set_blocked`]).

use std::any::Any;
use std::collections::BTreeSet;
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use clauseo_limits::{ProcessReaders, ReadFailure, ReadLimits, ReadOutcome};
use clauseo_process::{AsyncIoTimer, Launcher, Timer as _};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{AgentLaunch, Effective};
use zeroize::Zeroizing;

use super::hook::{Plans, setup_vars};
use super::keys::KeySlot;
use crate::dialogs::BoxFuture;

/// One hold on an agent's limits reads; dropping it ends the hold.
pub(crate) struct ReadHold {
    _guard: Box<dyn Any + Send>,
}

impl ReadHold {
    /// A hold that ends when `guard` drops.
    pub(crate) fn new(guard: impl Any + Send) -> Self {
        Self {
            _guard: Box::new(guard),
        }
    }
}

/// What the setup backend asks of the limits readers.
pub(crate) trait LeaveReaders: Send + Sync {
    /// Holds `agent`'s reads from this call on; the future resolves once none is running.
    fn hold(&self, agent: AgentKind) -> BoxFuture<'static, ReadHold>;
    /// Reads `agent` from `launch` from now on (`None`: no entry).
    fn set_entry(&self, agent: AgentKind, launch: Option<AgentLaunch>);
    /// The agents an unreadable policy blocks from now on: none of their reads goes out.
    fn set_blocked(&self, blocked: BTreeSet<AgentId>);
}

/// How long a limits read waits for the keys to load before it fails: the host's own bound on
/// `agent.keys_loading`.
pub(crate) const KEYS_HOLD: Duration = Duration::from_secs(12);

/// How often a waiting limits read looks at the key slot.
pub(crate) const KEYS_POLL: Duration = Duration::from_millis(100);

/// The host's limits reader: one `ProcessReaders`, whose entry is set on each entry written,
/// the key slot its reads wait for, and the agents an unreadable policy blocks.
pub(crate) struct SwappableReaders {
    readers: Arc<ProcessReaders<AsyncIoTimer>>,
    keys: KeySlot,
    blocked: Arc<RwLock<BTreeSet<AgentId>>>,
}

impl SwappableReaders {
    /// Readers for `effective`'s entries over `launcher`, with Clauseo's own homes forced and
    /// each read's variables from `keys` (no key until the setup thread has loaded them) and
    /// `plans`, as the launch hook makes them.
    pub(crate) fn new(
        effective: &Effective,
        launcher: Arc<dyn Launcher>,
        keys: KeySlot,
        plans: Plans,
    ) -> Self {
        let locations = effective.locations();
        let readers = ProcessReaders::new(
            effective,
            locations.agent_home(AgentKind::Codex.id()),
            launcher,
            AsyncIoTimer,
        )
        .with_claude_home(locations.agent_home(AgentKind::Claude.id()));
        let vars = Arc::clone(&keys);
        let readers = readers.with_launch_env(move |kind| read_vars(&vars, &plans, kind));
        Self {
            readers: Arc::new(readers),
            keys,
            blocked: Arc::default(),
        }
    }

    /// These readers with `blocked` the agents an unreadable policy blocks, as read at start.
    #[must_use]
    pub(crate) fn with_blocked(self, blocked: BTreeSet<AgentId>) -> Self {
        self.set_blocked(blocked);
        self
    }
}

/// The failure a read of `kind` ends with while an unreadable policy blocks it.
fn refuse_blocked(blocked: &RwLock<BTreeSet<AgentId>>, kind: AgentKind) -> Result<(), ReadFailure> {
    let blocked = blocked.read().unwrap_or_else(PoisonError::into_inner);
    if AgentId::new(kind.id()).is_ok_and(|agent| blocked.contains(&agent)) {
        return Err(ReadFailure::LaunchFailed(
            "the firm's policy could not be read".to_owned(),
        ));
    }
    Ok(())
}

/// Waits until `keys` has loaded, at most [`KEYS_HOLD`]: the failure a read of `kind` ends
/// with when they have not, or when its stored key is unreadable.
async fn keys_ready(keys: &KeySlot, kind: AgentKind) -> Result<(), ReadFailure> {
    let mut waited = Duration::ZERO;
    loop {
        if let Some(cache) = keys.get() {
            let unreadable = AgentId::new(kind.id()).is_ok_and(|agent| cache.unreadable(&agent));
            if unreadable {
                return Err(ReadFailure::LaunchFailed(
                    "the stored API key could not be read".to_owned(),
                ));
            }
            return Ok(());
        }
        if waited >= KEYS_HOLD {
            return Err(ReadFailure::LaunchFailed(
                "the stored API keys did not load".to_owned(),
            ));
        }
        AsyncIoTimer.after(KEYS_POLL).await;
        waited = waited.saturating_add(KEYS_POLL);
    }
}

impl ReadLimits for SwappableReaders {
    fn read(
        &self,
        agent: AgentKind,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ReadOutcome> + Send>> {
        let readers = Arc::clone(&self.readers);
        let keys = Arc::clone(&self.keys);
        let blocked = Arc::clone(&self.blocked);
        Box::pin(async move {
            let ready = keys_ready(&keys, agent).await;
            // After the wait, so a block that came meanwhile holds.
            if let Err(failure) = ready.and_then(|()| refuse_blocked(&blocked, agent)) {
                return ReadOutcome::Failed(failure);
            }
            readers.read(agent).await
        })
    }
}

impl LeaveReaders for SwappableReaders {
    fn hold(&self, agent: AgentKind) -> BoxFuture<'static, ReadHold> {
        let held = self.readers.hold(agent);
        Box::pin(async move { ReadHold::new(held.await) })
    }

    fn set_entry(&self, agent: AgentKind, launch: Option<AgentLaunch>) {
        self.readers.set_entry(agent, launch);
    }

    fn set_blocked(&self, blocked: BTreeSet<AgentId>) {
        *self.blocked.write().unwrap_or_else(PoisonError::into_inner) = blocked;
    }
}

/// The variables a limits read of `kind` adds: [`setup_vars`], moved, never copied, so each
/// value is zeroed when `limits` drops it (review L6).
pub(crate) fn read_vars(
    keys: &KeySlot,
    plans: &Plans,
    kind: AgentKind,
) -> Vec<(String, Zeroizing<String>)> {
    let Ok(agent) = AgentId::new(kind.id()) else {
        return Vec::new();
    };
    setup_vars(keys, plans, &agent, kind).into_iter().collect()
}

#[cfg(test)]
mod tests;
