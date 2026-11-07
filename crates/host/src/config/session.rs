//! The session key and the egress proxy seam the launch's browser entry asks.

use clauseo_protocol::{MatterId, ThreadId};

/// One agent session, as the egress proxy knows it: the matter, the thread, and the launch (the
/// launch prep's id, from the same counter as the agent's).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionKey {
    /// The thread's matter.
    pub matter: MatterId,
    /// The thread.
    pub thread: ThreadId,
    /// The launch.
    pub launch: u64,
}

impl SessionKey {
    /// The session's id, `<thread>-<launch>`: the name of its `run/` folder and agent-browser's
    /// session name.
    pub fn id(&self) -> String {
        format!("{}-{}", self.thread, self.launch)
    }
}

/// The egress proxy, as the host asks it (stage C's R17 implements it). Until it exists,
/// production passes none and the `browser` entry is left out (`NoProxy`).
pub trait EgressProxy: Send + Sync {
    /// The port of `session`'s browser listener, or none.
    fn port_for(&self, session: SessionKey) -> Option<u16>;
}

/// A proxy stand-in for tests: the same port for every session.
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, Copy)]
pub struct TestProxy(pub u16);

#[cfg(feature = "test-support")]
impl EgressProxy for TestProxy {
    fn port_for(&self, _session: SessionKey) -> Option<u16> {
        Some(self.0)
    }
}
