//! The free space two installs at once count on (decision 0033, "The disk need"). Each try's
//! check records what it needs ([`Reserve`]); a later check counts, besides its own need, what
//! every other running try still has to write: its recorded need less the bytes of its own files
//! now on the disk ([`crate::install::written`]: its downloads, its npm cache, its install
//! trees, and Node's archive and runtime when it fetches them), whatever phase it is in
//! (download, Node's unpack, `npm ci`), so nothing written is counted twice: once in the free
//! space read now, and again in the other's need. Free functions over [`Shared`] (one inherent
//! `impl` per type, `shared.rs`).

use std::sync::PoisonError;

use clauseo_protocol::AgentId;

use super::{Inner, Shared, Ticket};
use crate::config::AgentsConfig;
use crate::disk;

/// What one try's disk check counted on, by part: the agent's (its packages twice, the unpacked
/// tree, Node's archive and runtime when it fetches them, plus 10 %) and Python's, when this job
/// counted it.
#[derive(Debug, Default, Clone, Copy)]
pub(super) struct Reserve {
    /// The agent's part; 0 from its `Ready` on.
    pub(super) agent: u64,
    /// Whether the agent's part holds Node's: its archive and runtime are its files then.
    node: bool,
    python: u64,
    /// Python's bytes downloaded so far.
    pub(super) python_received: u64,
}

impl Reserve {
    /// What `agent`'s try still has to write: its agent part less the bytes of its files on
    /// the disk now, and Python's part less what it has downloaded, unless `with_python` is
    /// false (the asking job installs Python itself).
    fn left(&self, config: &AgentsConfig, agent: &AgentId, with_python: bool) -> u64 {
        let agent_left = if self.agent == 0 {
            0
        } else {
            let written = crate::install::written(config, agent, self.node);
            self.agent.saturating_sub(written)
        };
        let python_left = if with_python {
            self.python.saturating_sub(self.python_received)
        } else {
            0
        };
        agent_left.saturating_add(python_left)
    }
}

/// The disk check of a try of `ticket`'s job, which needs `need.0` bytes for its agent (`node`:
/// Node's archive and runtime included) and `need.1` for Python (0 when Python is installed or
/// this platform has none). Every other running try's remaining need is counted too
/// ([`Reserve::left`]), and Python only once: not here when another try counted it. The free
/// space is read after those, and the whole check holds `space`, so two tries starting together
/// never both count on the same bytes. `Err` with the bytes needed in all and the free bytes
/// (`None`: not readable, so not checked); else this try's need is recorded.
pub(crate) fn check_space(
    shared: &Shared,
    config: &AgentsConfig,
    ticket: &Ticket,
    need: (u64, u64),
    node: bool,
) -> Result<(), (u64, Option<u64>)> {
    let _one = shared.space.lock().unwrap_or_else(PoisonError::into_inner);
    let others = others(&shared.lock(), ticket);
    let python_counted = others.iter().any(|(_, reserve)| reserve.python > 0);
    let python = if python_counted { 0 } else { need.1 };
    let left = sum(others
        .iter()
        .map(|(id, reserve)| reserve.left(config, id, true)));
    let free = disk::free_for(config);
    let reserve = Reserve {
        agent: need.0,
        node,
        python,
        python_received: 0,
    };
    record(&mut shared.lock(), ticket, reserve, left, free)
}

/// Python's disk check in the job of `ticket`, the one installing it (`Shared::claim_python`):
/// `python` bytes, with every other running try's remaining need for its agent (a Python part
/// another try counted is this job's now, so not counted again). As [`check_space`].
pub(crate) fn check_python_space(
    shared: &Shared,
    config: &AgentsConfig,
    ticket: &Ticket,
    python: u64,
) -> Result<(), (u64, Option<u64>)> {
    let _one = shared.space.lock().unwrap_or_else(PoisonError::into_inner);
    let others = others(&shared.lock(), ticket);
    let left = sum(others
        .iter()
        .map(|(id, reserve)| reserve.left(config, id, false)));
    let free = disk::free_for(config);
    let reserve = Reserve {
        python,
        ..Reserve::default()
    };
    record(&mut shared.lock(), ticket, reserve, left, free)
}

/// Whether a job of this process is fetching Node now: its try's check counted Node's archive
/// and runtime, and its agent part is not done. A wait for `.lock-node` held by it lasts as long
/// as that fetch (`install/locks.rs`).
pub(crate) fn node_fetch_here(shared: &Shared) -> bool {
    shared
        .lock()
        .jobs
        .values()
        .any(|slot| slot.reserve.node && slot.reserve.agent > 0)
}

/// The bytes every running try but `ticket`'s still needs, for a `DiskFull` failure's need.
pub(crate) fn others_left(shared: &Shared, config: &AgentsConfig, ticket: &Ticket) -> u64 {
    let others = others(&shared.lock(), ticket);
    sum(others
        .iter()
        .map(|(id, reserve)| reserve.left(config, id, true)))
}

fn sum(parts: impl Iterator<Item = u64>) -> u64 {
    parts.fold(0, u64::saturating_add)
}

/// The agent and reserve of every running job but `ticket`'s.
fn others(inner: &Inner, ticket: &Ticket) -> Vec<(AgentId, Reserve)> {
    inner
        .jobs
        .values()
        .filter(|slot| slot.ticket.job != ticket.job)
        .map(|slot| (slot.ticket.agent.clone(), slot.reserve))
        .collect()
}

/// Checks `reserve`'s bytes, with `others` still needed by the other tries, against `free`, and
/// records `reserve` as `ticket`'s when they fit (or `free` is not known).
fn record(
    inner: &mut Inner,
    ticket: &Ticket,
    reserve: Reserve,
    others: u64,
    free: Option<u64>,
) -> Result<(), (u64, Option<u64>)> {
    let need = reserve
        .agent
        .saturating_add(reserve.python)
        .saturating_add(others);
    if free.is_some_and(|free| free < need) {
        return Err((need, free));
    }
    if let Some(slot) = inner.slot_mut(ticket) {
        slot.reserve = reserve;
    }
    Ok(())
}
