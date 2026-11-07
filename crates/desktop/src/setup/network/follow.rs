//! The setup's network thread: each settings read is resolved there, the OS asked about every
//! target at once within one budget ([`OS_LOOKUP_BUDGET`]: WPAD or a PAC script may be slow),
//! the result logged when it changed and handed to `Agents` and the launch hook's slot.
//!
//! The thread starts with the setup thread, before the keys load, so the OS lookups run beside
//! the keys' load and never on the window's start path. `Agents`, which starts after the keys,
//! is bound to it then ([`NetworkFollow::bind`]) and given the route resolved so far.
//!
//! The first route is awaited ([`NetworkFollow::ready`], at most until [`FIRST_ROUTE_WAIT`] after
//! the setup's start) before a command that reaches the network (an install, a retry, a sign-in)
//! goes to `Agents`: behind a proxy set only in the OS, a first try that
//! went direct would be refused by the firm's firewall and counted as a failed try. The start's
//! `agent.keys_loading` launch refusal is lifted only after it too (`refusal::lift_at_start`), so
//! no agent launched at start goes direct for its whole life.

use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError, mpsc};
use std::time::Duration;

use clauseo_agents::{Clock as _, Network, SystemClock};
use clauseo_process::{AsyncIoTimer, Timer as _};
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{Either, Shared, select};
use url::Url;

use super::{NetworkFacts, NetworkRoute, NetworkSlot, OsLookup, ProxyEnv, ask_all, os_targets};
use crate::bridge::lock;
use crate::setup::backend::SetupAgents;

/// The whole proxy resolution's budget, WPAD and PAC included: the longest a command that
/// reaches the network, or a launch held at start, waits for the first route after the setup's
/// start. Past it, it goes on with the route resolved so far (the settings' and the
/// environment's until the OS's lands).
pub(crate) const FIRST_ROUTE_WAIT: Duration = Duration::from_secs(5);

/// How long one resolve waits for the OS's answers, all asked at once: the budget less a margin
/// for handing the route on. A lookup past it counts as direct in that route.
pub(crate) const OS_LOOKUP_BUDGET: Duration = Duration::from_millis(4_750);

/// Takes each settings read in: resolves the route and hands it on ([`Follower::take`]).
pub(crate) struct NetworkFollow {
    sink: Box<dyn Fn(NetworkFacts) + Send + Sync>,
    /// Ends once the first route has been handed on.
    first: Shared<oneshot::Receiver<()>>,
    /// When the setup started (`SystemClock`'s monotonic reading): each wait for the first
    /// route counts from it.
    started: Duration,
    /// The follower, for [`NetworkFollow::bind`]; `None` for the default one.
    follower: Option<Arc<Mutex<Follower>>>,
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
            started: SystemClock.monotonic(),
            follower: None,
        }
    }
}

impl NetworkFollow {
    /// Hands `facts` on: the next route is made from them.
    pub(crate) fn update(&self, facts: NetworkFacts) {
        (self.sink)(facts);
    }

    /// Gives every route from now on to `agents`, and at once the one resolved so far, if any
    /// (before any command can reach `agents`, so it comes first in their queue).
    pub(crate) fn bind(&self, agents: Arc<dyn SetupAgents>) {
        if let Some(follower) = &self.follower {
            lock(follower).bind(agents);
        }
    }

    /// Waits until the first route has been handed on, at most until [`FIRST_ROUTE_WAIT`]
    /// after the setup's start.
    pub(crate) async fn ready(&self) {
        self.ready_within(FIRST_ROUTE_WAIT).await;
    }

    /// Waits until the first route has reached `Agents` and the launch hook, at most until
    /// `bound` after the setup's start: past it, what goes on takes the route resolved so far.
    pub(crate) async fn ready_within(&self, bound: Duration) {
        let first = self.first.clone();
        let left = (self.started + bound).saturating_sub(SystemClock.monotonic());
        let bound = AsyncIoTimer.after(left);
        if let Either::Right(_) = select(pin!(first), pin!(bound)).await {
            tracing::warn!("the OS proxy lookup is still running; the command goes on without it");
        }
    }

    /// One that resolves on the caller's thread, started now (tests).
    #[cfg(test)]
    pub(crate) fn inline(mut follower: Follower) -> Self {
        let first = follower.first_signal();
        let follower = Arc::new(Mutex::new(follower));
        let taker = Arc::clone(&follower);
        Self {
            sink: Box::new(move |facts| lock(&taker).take(&facts)),
            first,
            started: SystemClock.monotonic(),
            follower: Some(follower),
        }
    }
}

/// What makes a route from a read: the environment and the OS, read at each one.
#[derive(Clone)]
struct Resolver {
    env: Arc<dyn Fn() -> ProxyEnv + Send + Sync>,
    os: Arc<OsLookup>,
    os_fallback: bool,
}

impl Resolver {
    /// The route for `facts`, the OS asked about every target at once, within
    /// [`OS_LOOKUP_BUDGET`].
    fn resolve(&self, facts: &NetworkFacts) -> NetworkRoute {
        let env = (self.env)();
        let targets = os_targets(facts, &env, self.os_fallback);
        let answers = ask_all(targets, &self.os, OS_LOOKUP_BUDGET);
        let answered = |target: &Url| answers.get(target).cloned().flatten();
        super::resolve(facts, &env, self.os_fallback, &answered)
    }
}

/// What resolves each read and hands the route to `Agents` and the slot.
pub(crate) struct Follower {
    resolver: Resolver,
    slot: NetworkSlot,
    /// `None` until bound ([`NetworkFollow::bind`]).
    agents: Option<Arc<dyn SetupAgents>>,
    /// What `Agents` was last given; `None` before the first, which always goes.
    pushed: Option<Network>,
    /// The network of the last route handed on, for `Agents` bound after it.
    last: Option<Network>,
    /// What the log last said.
    logged: Option<String>,
    /// Told once the first route has been handed on.
    landed: Option<oneshot::Sender<()>>,
}

impl Follower {
    /// Over `slot` and `agents`, with `env` and `os` read at each take (tests).
    #[cfg(test)]
    pub(crate) fn new(
        slot: NetworkSlot,
        agents: Arc<dyn SetupAgents>,
        env: impl Fn() -> ProxyEnv + Send + Sync + 'static,
        os: impl Fn(&Url) -> Option<String> + Send + Sync + 'static,
        os_fallback: bool,
    ) -> Self {
        let mut follower = Self::unbound(slot, env, os, os_fallback);
        follower.agents = Some(agents);
        follower
    }

    /// [`Follower::new`] with no `Agents` yet: [`NetworkFollow::bind`] gives it.
    pub(crate) fn unbound(
        slot: NetworkSlot,
        env: impl Fn() -> ProxyEnv + Send + Sync + 'static,
        os: impl Fn(&Url) -> Option<String> + Send + Sync + 'static,
        os_fallback: bool,
    ) -> Self {
        Self {
            resolver: Resolver {
                env: Arc::new(env),
                os: Arc::new(os),
                os_fallback,
            },
            slot,
            agents: None,
            pushed: None,
            last: None,
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

    /// Resolves `facts` and hands the route on ([`Follower::hand`]).
    pub(crate) fn take(&mut self, facts: &NetworkFacts) {
        let route = self.resolver.resolve(facts);
        self.hand(route);
    }

    /// Logs the source and proxies when they changed (and the first time), gives `Agents` the
    /// network when it changed, and puts the route in the slot.
    fn hand(&mut self, route: NetworkRoute) {
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
        self.last = Some(route.agents_network());
        self.push();
        *self.slot.write().unwrap_or_else(PoisonError::into_inner) = route;
        if let Some(landed) = self.landed.take() {
            // No one waiting is fine.
            let _told = landed.send(());
        }
    }

    /// Gives `agents` the last network, if any, and every one after it.
    fn bind(&mut self, agents: Arc<dyn SetupAgents>) {
        self.agents = Some(agents);
        self.push();
    }

    /// Gives `Agents`, when bound, the last network when it is not what it has.
    fn push(&mut self) {
        let (Some(agents), Some(network)) = (&self.agents, &self.last) else {
            return;
        };
        if self.pushed.as_ref() != Some(network) {
            agents.set_network(network.clone());
            self.pushed = Some(network.clone());
        }
    }
}

/// Starts the `clauseo-setup-network` thread over `follower`: each read handed to the
/// returned [`NetworkFollow`] is resolved there (the newest of any queued), off the threads
/// that follow the settings and the start's, since the OS lookups may wait on WPAD or a PAC
/// script. With no thread, reads are resolved on the caller's. Every wait for the first route
/// ends at most [`FIRST_ROUTE_WAIT`] after `started`, the setup's start (`SystemClock`'s
/// monotonic reading).
pub(crate) fn follow(mut follower: Follower, started: Duration) -> NetworkFollow {
    let first = follower.first_signal();
    let resolver = follower.resolver.clone();
    let (send, receive) = mpsc::channel::<NetworkFacts>();
    let follower = Arc::new(Mutex::new(follower));
    let on_thread = Arc::clone(&follower);
    let spawned = std::thread::Builder::new()
        .name("clauseo-setup-network".to_owned())
        .spawn(move || {
            while let Ok(mut facts) = receive.recv() {
                while let Ok(newer) = receive.try_recv() {
                    facts = newer;
                }
                // Resolved with no lock held, so a bind never waits on the OS.
                let route = resolver.resolve(&facts);
                lock(&on_thread).hand(route);
            }
        });
    let sink: Box<dyn Fn(NetworkFacts) + Send + Sync> = match spawned {
        Ok(_thread) => Box::new(move |facts| {
            // Gone only with the thread, which ends with the app.
            let _sent = send.send(facts);
        }),
        Err(err) => {
            tracing::warn!(%err, "no network thread: the proxy is resolved inline");
            let taker = Arc::clone(&follower);
            Box::new(move |facts| lock(&taker).take(&facts))
        }
    };
    NetworkFollow {
        sink,
        first,
        started,
        follower: Some(follower),
    }
}

#[cfg(test)]
mod tests;
