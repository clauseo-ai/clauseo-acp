//! The session key the egress proxy (`config/egress.rs`) and the launch's browser entry use.

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
