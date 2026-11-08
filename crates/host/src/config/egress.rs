//! The egress proxy's seam ([`EgressProxy`], S16 and R17), its test stand-in, and how long a
//! session's removal waits before its proxy closes. Kept apart from `config.rs`, whose
//! `HostConfig` holds them.

use std::ffi::OsString;
use std::fmt;
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
/// host thread. A jailed launch (R17 PR-D) also takes the agent listener's port and variables
/// ([`EgressProxy::agent_listener`]).
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
    /// `session`'s agent listener (R17 PR-D): its port, for the jail's `proxy_ports`, and the
    /// variables that point the agent at it (`clauseo_sandbox::ProxyHandle::env`, which carry
    /// the proxy's token); none for a session it never opened.
    fn agent_listener(&self, session: SessionKey) -> Option<AgentListener>;
    /// Stops `session`'s proxy, if it has one: a session it never opened is a no-op.
    fn close(&self, session: SessionKey);
}

/// A session proxy's agent listener ([`EgressProxy::agent_listener`]): its port and the agent's
/// proxy variables, `Some` to set and `None` to remove. The values carry the proxy's token, so
/// `Debug` shows the names only.
#[derive(Clone)]
#[non_exhaustive]
pub struct AgentListener {
    /// The listener's loopback port.
    pub port: u16,
    /// The agent's proxy variables.
    pub env: Vec<(OsString, Option<OsString>)>,
}

impl AgentListener {
    /// The listener on `port`, pointed at by `env`.
    #[must_use]
    pub fn new(port: u16, env: Vec<(OsString, Option<OsString>)>) -> Self {
        Self { port, env }
    }
}

impl fmt::Debug for AgentListener {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let names: Vec<&OsString> = self.env.iter().map(|(name, _)| name).collect();
        f.debug_struct("AgentListener")
            .field("port", &self.port)
            .field("env", &names)
            .finish()
    }
}

/// A proxy stand-in for tests: the same port for every session, for the browser and the agent
/// listener, with `HTTPS_PROXY` naming it and `ALL_PROXY` removed; `open` and `close` do
/// nothing.
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

    fn agent_listener(&self, _session: SessionKey) -> Option<AgentListener> {
        let url = OsString::from(format!("http://clauseo:test@127.0.0.1:{}", self.0));
        Some(AgentListener::new(
            self.0,
            vec![
                (OsString::from("HTTPS_PROXY"), Some(url)),
                (OsString::from("ALL_PROXY"), None),
            ],
        ))
    }

    fn close(&self, _session: SessionKey) {}
}
