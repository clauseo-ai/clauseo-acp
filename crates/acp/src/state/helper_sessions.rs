//! The helpers codex-acp announced on this connection (`subagent_spawned`), by the session id
//! their own frames carry, each with its root: the session the lawyer talks to. Empty unless
//! `ConnectOptions::helper_sessions` is on, since only then is a `subagent_spawned` read.
//!
//! Only a session this connection started is a root, and a helper keeps the root it was first
//! announced under, so the agent cannot grow the map with sessions nobody forgets, nor move a
//! helper's requests to another root's cancel. An entry stays after the helper ends, so its late
//! frames still find the root. It is forgotten with its root's entry in `State::sessions`: when
//! the root is started again (`session/new`, `session/load`, `session/resume`; a load's replay
//! announces its helpers again) or dropped (a failed load or resume), and with the connection.
//! A session started under an id once announced as a helper stops being one.

use std::collections::BTreeMap;

use clauseo_protocol::{AgentSessionId, CompactString};

/// Helper session id → its root session.
#[derive(Debug, Default)]
pub(crate) struct HelperSessions(BTreeMap<CompactString, AgentSessionId>);

impl HelperSessions {
    /// The root of helper session `id`, when `id` was announced.
    pub(crate) fn root_of(&self, id: &str) -> Option<&AgentSessionId> {
        self.0.get(id)
    }

    /// Records helper session `helper` under `root`, replacing what it had.
    pub(crate) fn insert(&mut self, helper: CompactString, root: AgentSessionId) {
        self.0.insert(helper, root);
    }

    /// Forgets helper session `id`, if it was one.
    pub(crate) fn forget(&mut self, id: &str) {
        self.0.remove(id);
    }

    /// Forgets every helper whose root is `root`.
    pub(crate) fn forget_root(&mut self, root: &str) {
        self.0.retain(|_, known| known.as_str() != root);
    }
}
