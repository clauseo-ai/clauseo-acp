//! The connector state's two limits on the actor: the one-a-minute account read ([`Reads`])
//! and the `command_id`s connector commands used ([`UsedIds`]).

use std::collections::{BTreeSet, VecDeque};
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
