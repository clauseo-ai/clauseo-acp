//! The limits readers desktop gives the host (`HostConfig::with_limits_reader`), so the setup
//! backend can hold an agent's reads across a login change and swap in each entry it writes
//! (decision record 0048, the plan's ruling 7):
//!
//! - built over the host's launcher, with Clauseo's own homes forced (`CODEX_HOME` and, through
//!   `ProcessReaders::with_claude_home`, `CLAUDE_CONFIG_DIR`, ruling 6), and the stored key
//!   asked of the [`KeySlot`] at each read (`with_launch_env`);
//! - [`SwappableReaders::set_entry`] rebuilds them with `ProcessReaders::with_entry` after each
//!   `EntryWrite::Written`; the copy shares the gates, so a hold taken before survives it;
//! - [`ReadHold`] is the hold the backend keeps across the leave order, as [`LeaveReaders`]
//!   names it, so the backend's tests play it with a fake.

use std::any::Any;
use std::sync::{Arc, PoisonError, RwLock};

use clauseo_limits::{ProcessReaders, ReadLimits, ReadOutcome};
use clauseo_process::{AsyncIoTimer, Launcher};
use clauseo_protocol::{AgentId, AgentKind};
use clauseo_settings::{AgentLaunch, Effective};

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

/// The host's limits reader: one `ProcessReaders`, swapped whole on each entry written.
pub(crate) struct SwappableReaders(RwLock<Arc<ProcessReaders<AsyncIoTimer>>>);

impl SwappableReaders {
    /// Readers for `effective`'s entries over `launcher`, with Clauseo's own homes forced and
    /// each read's key from `keys` (none until the setup thread has loaded them).
    pub(crate) fn new(effective: &Effective, launcher: Arc<dyn Launcher>, keys: KeySlot) -> Self {
        let locations = effective.locations();
        let readers = ProcessReaders::new(
            effective,
            locations.agent_home(AgentKind::Codex.id()),
            launcher,
            AsyncIoTimer,
        )
        .with_claude_home(locations.agent_home(AgentKind::Claude.id()))
        .with_launch_env(move |kind| {
            let agent = AgentId::new(kind.id()).ok()?;
            let key = keys.get()?.get(&agent)?;
            Some(key.as_str().to_owned())
        });
        Self::from_readers(readers)
    }

    /// Swaps over `readers`.
    pub(crate) fn from_readers(readers: ProcessReaders<AsyncIoTimer>) -> Self {
        Self(RwLock::new(Arc::new(readers)))
    }

    fn current(&self) -> Arc<ProcessReaders<AsyncIoTimer>> {
        Arc::clone(&self.0.read().unwrap_or_else(PoisonError::into_inner))
    }
}

impl ReadLimits for SwappableReaders {
    fn read(
        &self,
        agent: AgentKind,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ReadOutcome> + Send>> {
        self.current().read(agent)
    }
}

impl LeaveReaders for SwappableReaders {
    fn hold(&self, agent: AgentKind) -> BoxFuture<'static, ReadHold> {
        let held = self.current().hold(agent);
        Box::pin(async move { ReadHold::new(held.await) })
    }

    fn set_entry(&self, agent: AgentKind, launch: Option<AgentLaunch>) {
        let mut readers = self.0.write().unwrap_or_else(PoisonError::into_inner);
        let rebuilt = readers.with_entry(agent, launch);
        *readers = Arc::new(rebuilt);
    }
}

#[cfg(test)]
mod tests;
