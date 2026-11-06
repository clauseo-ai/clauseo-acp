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

use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use clauseo_limits::{ProcessReaders, ReadFailure, ReadLimits, ReadOutcome};
use clauseo_process::{AsyncIoTimer, Launcher, Timer as _};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{AgentLaunch, Effective};

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
}

/// How long a limits read waits for the keys to load before it fails: the host's own bound on
/// `agent.keys_loading`.
pub(crate) const KEYS_HOLD: Duration = Duration::from_secs(12);

/// How often a waiting limits read looks at the key slot.
pub(crate) const KEYS_POLL: Duration = Duration::from_millis(100);

/// The host's limits reader: one `ProcessReaders`, whose entry is set on each entry written,
/// and the key slot its reads wait for.
pub(crate) struct SwappableReaders {
    readers: Arc<ProcessReaders<AsyncIoTimer>>,
    keys: KeySlot,
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
        }
    }
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
        Box::pin(async move {
            if let Err(failure) = keys_ready(&keys, agent).await {
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
}

/// The variables a limits read of `kind` adds: [`setup_vars`], copied into the plain strings
/// `limits` takes, which it drops once the read's process is spawned.
pub(crate) fn read_vars(keys: &KeySlot, plans: &Plans, kind: AgentKind) -> Vec<(String, String)> {
    let Ok(agent) = AgentId::new(kind.id()) else {
        return Vec::new();
    };
    setup_vars(keys, plans, &agent, kind)
        .iter()
        .map(|(name, value)| (name.clone(), value.as_str().to_owned()))
        .collect()
}

#[cfg(test)]
mod tests;
