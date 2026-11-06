//! The setup's network thread: each settings read is resolved there (an OS lookup may wait on
//! WPAD or a PAC script), the result logged when it changed and handed to `Agents` and the
//! launch hook's slot.
//!
//! The first route is awaited ([`NetworkFollow::ready`], at most [`FIRST_ROUTE_WAIT`]) before
//! a command that reaches the network (an install, a retry, a sign-in) goes to `Agents`: behind
//! a proxy set only in the OS, a first try that went direct would be refused by the firm's
//! firewall and counted as a failed try.

use std::pin::pin;
use std::sync::{Arc, PoisonError, mpsc};
use std::time::Duration;

use clauseo_agents::Network;
use clauseo_process::{AsyncIoTimer, Timer as _};
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{Either, Shared, select};
use url::Url;

use super::{NetworkFacts, NetworkSlot, OsLookup, ProxyEnv, resolve};
use crate::setup::backend::SetupAgents;

/// The longest a command that reaches the network waits for the first route: past it, it goes
/// on with the settings' and the environment's (the OS lookups are bounded in the sandbox, but
/// several of them in a row may take longer).
pub(crate) const FIRST_ROUTE_WAIT: Duration = Duration::from_secs(20);

/// Takes each settings read in: resolves the route and hands it on ([`Follower::take`]).
pub(crate) struct NetworkFollow {
    sink: Box<dyn Fn(NetworkFacts) + Send + Sync>,
    /// Ends once the first route has been handed to `Agents`.
    first: Shared<oneshot::Receiver<()>>,
}

impl Default for NetworkFollow {
    /// One that takes nothing in and is ready at once (the backend's tests that do not follow
    /// the network).
    fn default() -> Self {
        let (landed, first) = oneshot::channel();
        // The receiver is held just below.
        let _sent = landed.send(());
        Self {
            sink: Box::new(|_facts| {}),
            first: first.shared(),
        }
    }
}

impl NetworkFollow {
    /// Hands `facts` on: the next route is made from them.
    pub(crate) fn update(&self, facts: NetworkFacts) {
        (self.sink)(facts);
    }

    /// Waits until the first route has reached `Agents`, at most [`FIRST_ROUTE_WAIT`].
    pub(crate) async fn ready(&self) {
        let first = self.first.clone();
        let bound = AsyncIoTimer.after(FIRST_ROUTE_WAIT);
        if let Either::Right(_) = select(pin!(first), pin!(bound)).await {
            tracing::warn!("the OS proxy lookup is still running; the command goes on without it");
        }
    }

    /// One that resolves on the caller's thread (tests).
    #[cfg(test)]
    pub(crate) fn inline(mut follower: Follower) -> Self {
        let first = follower.first_signal();
        let follower = std::sync::Mutex::new(follower);
        Self {
            sink: Box::new(move |facts| {
                crate::bridge::lock(&follower).take(&facts);
            }),
            first,
        }
    }
}

/// What resolves each read and hands the route to `Agents` and the slot.
pub(crate) struct Follower {
    slot: NetworkSlot,
    agents: Arc<dyn SetupAgents>,
    env: Box<dyn Fn() -> ProxyEnv + Send>,
    os: Box<OsLookup>,
    os_fallback: bool,
    /// What `Agents` was last given; `None` before the first, which always goes.
    pushed: Option<Network>,
    /// What the log last said.
    logged: Option<String>,
    /// Told once the first route has been handed on.
    landed: Option<oneshot::Sender<()>>,
}

impl Follower {
    /// Over `slot` and `agents`, with `env` and `os` read at each take.
    pub(crate) fn new(
        slot: NetworkSlot,
        agents: Arc<dyn SetupAgents>,
        env: impl Fn() -> ProxyEnv + Send + 'static,
        os: impl Fn(&Url) -> Option<String> + Send + Sync + 'static,
        os_fallback: bool,
    ) -> Self {
        Self {
            slot,
            agents,
            env: Box::new(env),
            os: Box::new(os),
            os_fallback,
            pushed: None,
            logged: None,
            landed: None,
        }
    }

    /// What ends once the first route has been handed on.
    fn first_signal(&mut self) -> Shared<oneshot::Receiver<()>> {
        let (landed, first) = oneshot::channel();
        self.landed = Some(landed);
        first.shared()
    }

    /// Resolves `facts`; logs the source and proxies when they changed (and the first time),
    /// gives `Agents` the network when it changed, and puts the route in the slot.
    pub(crate) fn take(&mut self, facts: &NetworkFacts) {
        let route = resolve(facts, &(self.env)(), self.os_fallback, &*self.os);
        let summary = route.summary();
        if self.logged.as_ref() != Some(&summary) {
            // `Upstream`'s `Debug` shows a fixed URL's scheme, host and port only.
            let upstream = route.upstream();
            tracing::info!(route = %summary, sandbox_upstream = ?upstream, "the agents' proxy");
            if route.setting_is_https() {
                tracing::warn!(
                    "network.upstream_proxy is an https:// proxy: installs and sign-ins use it, \
                     but the sandbox's egress proxy (tiers A and B) chains only to http:// \
                     proxies, so a sandboxed agent's traffic cannot go through it"
                );
            }
            self.logged = Some(summary);
        }
        let network = route.agents_network();
        if self.pushed.as_ref() != Some(&network) {
            self.agents.set_network(network.clone());
            self.pushed = Some(network);
        }
        *self.slot.write().unwrap_or_else(PoisonError::into_inner) = route;
        if let Some(landed) = self.landed.take() {
            // No one waiting is fine.
            let _told = landed.send(());
        }
    }
}

/// Starts the `clauseo-setup-network` thread over `follower`: each read handed to the
/// returned [`NetworkFollow`] is resolved there (the newest of any queued), off the threads
/// that follow the settings, since an OS lookup may wait on WPAD or a PAC script. With no
/// thread, reads are resolved on the caller's.
pub(crate) fn follow(mut follower: Follower) -> NetworkFollow {
    let first = follower.first_signal();
    let (send, receive) = mpsc::channel::<NetworkFacts>();
    let follower = Arc::new(std::sync::Mutex::new(follower));
    let on_thread = Arc::clone(&follower);
    let spawned = std::thread::Builder::new()
        .name("clauseo-setup-network".to_owned())
        .spawn(move || {
            while let Ok(mut facts) = receive.recv() {
                while let Ok(newer) = receive.try_recv() {
                    facts = newer;
                }
                crate::bridge::lock(&on_thread).take(&facts);
            }
        });
    match spawned {
        Ok(_thread) => NetworkFollow {
            sink: Box::new(move |facts| {
                // Gone only with the thread, which ends with the app.
                let _sent = send.send(facts);
            }),
            first,
        },
        Err(err) => {
            tracing::warn!(%err, "no network thread: the proxy is resolved inline");
            NetworkFollow {
                sink: Box::new(move |facts| crate::bridge::lock(&follower).take(&facts)),
                first,
            }
        }
    }
}

#[cfg(test)]
mod tests;
