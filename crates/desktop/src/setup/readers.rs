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

use std::any::Any;
use std::sync::Arc;

use clauseo_limits::{ProcessReaders, ReadLimits, ReadOutcome};
use clauseo_process::{AsyncIoTimer, Launcher};
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

/// The host's limits reader: one `ProcessReaders`, whose entry is set on each entry written.
pub(crate) struct SwappableReaders(ProcessReaders<AsyncIoTimer>);

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
        .with_claude_home(locations.agent_home(AgentKind::Claude.id()))
        .with_launch_env(move |kind| read_vars(&keys, &plans, kind));
        Self::from_readers(readers)
    }

    /// Over `readers`.
    pub(crate) fn from_readers(readers: ProcessReaders<AsyncIoTimer>) -> Self {
        Self(readers)
    }
}

impl ReadLimits for SwappableReaders {
    fn read(
        &self,
        agent: AgentKind,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = ReadOutcome> + Send>> {
        self.0.read(agent)
    }
}

impl LeaveReaders for SwappableReaders {
    fn hold(&self, agent: AgentKind) -> BoxFuture<'static, ReadHold> {
        let held = self.0.hold(agent);
        Box::pin(async move { ReadHold::new(held.await) })
    }

    fn set_entry(&self, agent: AgentKind, launch: Option<AgentLaunch>) {
        self.0.set_entry(agent, launch);
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
