//! The connector state's limits on the actor: the one-a-minute account read ([`Reads`]), the
//! reads after relays' paid `recharge` results ([`Recharged`]) and the `command_id`s connector
//! commands used ([`UsedIds`]).

use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use clauseo_protocol::CommandId;

use super::{READ_INTERVAL, USED_IDS};

/// The one-a-minute account read: when the last began, and whether one is in flight.
#[derive(Debug, Default)]
pub(super) struct Reads {
    pub(super) last: Option<Duration>,
    pub(super) in_flight: bool,
}

impl Reads {
    /// Takes the read slot at `now` when no read is in flight and the last began more than
    /// [`READ_INTERVAL`] ago.
    pub(super) fn take(&mut self, now: Duration) -> bool {
        let due = self
            .last
            .is_none_or(|last| now.saturating_sub(last) > READ_INTERVAL);
        if self.in_flight || !due {
            return false;
        }
        self.in_flight = true;
        self.last = Some(now);
        true
    }
}

/// The reads after relays' paid `recharge` results (`Job::Recharged`, at once and outside
/// [`Reads`]), one at a time, and the balance epoch. An agent that repeats `recharge` on a paid
/// recharge must not force an account read per call.
#[derive(Debug, Default)]
pub(super) struct Recharged {
    pub(super) in_flight: bool,
    /// The paid result the last read was started for; a resumed charge's known balance clears it.
    pub(super) last: Option<ReadFor>,
    /// A paid result that wanted a read while one was in flight (older than what it reports):
    /// read for once that one ends.
    pub(super) again: Option<ReadFor>,
    /// Bumped by a relay when a paid result resumed a charged call, before it drops the account
    /// copy (`RelayEvents::resumed_charge`): a job records it when it starts and right before it
    /// reads the account, and what it read once the epoch moved is not applied, being from
    /// before that charge. Shared with the relays and the jobs, as `credentials` is.
    pub(super) epoch: Arc<AtomicU64>,
}

impl Recharged {
    /// No read in flight, sharing the relays' balance epoch when connectors are wired.
    pub(super) fn sharing(epoch: Option<Arc<AtomicU64>>) -> Self {
        Self {
            epoch: epoch.unwrap_or_default(),
            ..Self::default()
        }
    }
}

/// The paid result a read is for: its `recharge_id`, if any, and whether it resumed a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ReadFor {
    pub(super) recharge: Option<String>,
    pub(super) resumed: bool,
}

impl ReadFor {
    /// Whether this result repeats the one `last` read for: a plain one for the same recharge
    /// (none never repeats). A resumed call is new spend, so it never repeats.
    pub(super) fn repeats(&self, last: &ReadFor) -> bool {
        !self.resumed && self.recharge.is_some() && self.recharge == last.recharge
    }
}

/// The `command_id`s connector commands used, oldest first, at most [`USED_IDS`]. Connector
/// commands store no receipt, so without this a reused id would run again, and its body's hash
/// would not tell two `AddApiKey`s apart (each key encodes as `[redacted]`): a reused id is
/// `command.conflict` instead, whatever the command, while the host runs.
#[derive(Debug, Default)]
pub(super) struct UsedIds {
    order: VecDeque<CommandId>,
    set: BTreeSet<CommandId>,
}

impl UsedIds {
    pub(super) fn contains(&self, id: CommandId) -> bool {
        self.set.contains(&id)
    }

    pub(super) fn insert(&mut self, id: CommandId) {
        if self.set.insert(id) {
            self.order.push_back(id);
        }
        while self.order.len() > USED_IDS {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
    }
}
