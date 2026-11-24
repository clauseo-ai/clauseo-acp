//! The helpers codex-acp announced on this connection (`subagent_spawned`), by the session id
//! their own frames carry, each with its root (the session the lawyer talks to) and how it
//! ended. Empty unless `ConnectOptions::helper_sessions` is on, since only then is a
//! `subagent_spawned` read.
//!
//! Only a session this connection started is a root, a helper is announced once, and a root
//! takes at most [`MAX_PER_ROOT`] announcements between two starts, so the agent cannot grow
//! the map with sessions nobody forgets, nor move a helper's requests to another root's cancel.
//! An entry stays after the helper ends, so its late frames still find the root. It is
//! forgotten with its root's entry in `State::sessions`: when the root is started again
//! (`session/new`, `session/load`, `session/resume`; a load's replay announces its helpers
//! again) or dropped (a failed load or resume), and with the connection. A session started
//! under an id once announced as a helper stops being one.

use std::collections::BTreeMap;

use clauseo_protocol::{AgentSessionId, CompactString, TurnId};

/// The most helpers one root session may announce between two starts of it. Ended helpers count
/// too, since their entries stay until the root is started again or dropped: this bounds the
/// map's memory. A `subagent_spawned` past it is refused. The recordings have at most four per
/// root (T1.jsonl).
pub(crate) const MAX_PER_ROOT: usize = 1024;

/// Helper session id → its root session and its end.
#[derive(Debug, Default)]
pub(crate) struct HelperSessions(BTreeMap<CompactString, Helper>);

#[derive(Debug)]
struct Helper {
    root: AgentSessionId,
    end: Option<End>,
}

/// That an announced helper ended (its `subagent_state_update`, whatever the state), so nothing
/// it sends afterwards belongs to the root's current turn. Holds the root's turn when the end
/// came (its origin was `Turn` or `Late` of it), `None` when the root had no turn (`Idle`,
/// `Replay`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct End(pub(crate) Option<TurnId>);

impl HelperSessions {
    /// The root of helper session `id`, when `id` was announced.
    pub(crate) fn root_of(&self, id: &str) -> Option<&AgentSessionId> {
        self.0.get(id).map(|helper| &helper.root)
    }

    /// Helper session `id`'s end; `None` while it runs, or when `id` was not announced.
    pub(crate) fn end_of(&self, id: &str) -> Option<End> {
        self.0.get(id).and_then(|helper| helper.end)
    }

    /// Whether `root` has had [`MAX_PER_ROOT`] helpers announced (running or ended) since it
    /// was last started.
    pub(crate) fn full(&self, root: &str) -> bool {
        let under = self
            .0
            .values()
            .filter(|helper| helper.root.as_str() == root);
        under.count() >= MAX_PER_ROOT
    }

    /// Records helper session `helper` under `root`, running, replacing what it had.
    pub(crate) fn insert(&mut self, helper: CompactString, root: AgentSessionId) {
        self.0.insert(helper, Helper { root, end: None });
    }

    /// Marks helper session `id` ended, if it was announced.
    pub(crate) fn end(&mut self, id: &str, end: End) {
        if let Some(helper) = self.0.get_mut(id) {
            helper.end = Some(end);
        }
    }

    /// Forgets helper session `id`, if it was one.
    pub(crate) fn forget(&mut self, id: &str) {
        self.0.remove(id);
    }

    /// Forgets every helper whose root is `root`.
    pub(crate) fn forget_root(&mut self, root: &str) {
        self.0.retain(|_, helper| helper.root.as_str() != root);
    }
}
