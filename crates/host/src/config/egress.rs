//! The egress proxy's seam ([`EgressProxy`], S16 and R17), its test stand-in, and how long a
//! session's removal waits before its proxy closes. Kept apart from `config.rs`, whose
//! `HostConfig` holds them.

use std::time::Duration;

use super::SessionKey;

/// How long a session's run folder removal waits, off the actor, for the agent's tree and its
/// Chrome to be gone before it frees the session's proxy, relay and ports
/// (`launch_checks::straggler`). Past it they stay held until the host stops.
pub const DEFAULT_TREE_WAIT: Duration = Duration::from_secs(20);

/// The egress proxy, one per agent launch (S16); the desktop's is over `start_proxy`. With none
/// set, the `browser` entry is left out (`NoProxy`). The host opens a session's proxy on the
/// launch-prep thread once the session's Chrome and browser claims are found, and closes it off
/// the actor when the session's last `EgressGuard` drops: in the run folder's removal
/// (`launch_checks::discard`) once the agent's tree and its Chrome are gone, or at host stop
/// for a session held past [`DEFAULT_TREE_WAIT`]. Both calls may block; neither runs on the
/// host thread. R17 PR-D adds `env`.
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
