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

/// The egress proxy, one per agent launch (S16); the desktop's is over `start_proxy`. With none
/// set, the `browser` entry is left out (`NoProxy`). The host opens a session's proxy on the
/// launch-prep thread when the session can get the browser, and closes it off the actor in the
/// run folder's removal (`launch_checks::discard`), once the agent's tree was killed or it never
/// started. Both calls may block; neither runs on the host thread. R17 PR-D adds `env`.
pub trait EgressProxy: Send + Sync {
    /// Starts `session`'s proxy, at most once per launch. When a jail exists (R17 PR-D), its
    /// ports go into that launch's policy as `proxy_ports`, so it opens before the policy is
    /// built. An error leaves the browser out (`NoProxy`); the launch goes on.
    ///
    /// # Errors
    ///
    /// The proxy did not start (`clauseo_sandbox::Error::ProxyStart` or `ProxyConfig`).
    fn open(
        &self,
        session: SessionKey,
        config: clauseo_sandbox::ProxyConfig,
    ) -> Result<(), clauseo_sandbox::Error>;
    /// The port of `session`'s browser listener, or none.
    fn port_for(&self, session: SessionKey) -> Option<u16>;
    /// Stops `session`'s proxy, if it has one: a session it never opened is a no-op.
    fn close(&self, session: SessionKey);
}

/// A proxy stand-in for tests: the same port for every session; `open` and `close` do nothing.
#[cfg(feature = "test-support")]
#[derive(Debug, Clone, Copy)]
pub struct TestProxy(pub u16);

#[cfg(feature = "test-support")]
impl EgressProxy for TestProxy {
    fn open(
        &self,
        _session: SessionKey,
        _config: clauseo_sandbox::ProxyConfig,
    ) -> Result<(), clauseo_sandbox::Error> {
        Ok(())
    }

    fn port_for(&self, _session: SessionKey) -> Option<u16> {
        Some(self.0)
    }

    fn close(&self, _session: SessionKey) {}
}
