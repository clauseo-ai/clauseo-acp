//! The setup's network thread: each settings read is resolved there, the OS asked about every
//! target at once within one budget ([`OS_LOOKUP_BUDGET`]: WPAD or a PAC script may be slow),
//! the result logged when it changed and handed to `Agents` and the launch hook's slot.
//!
//! The thread starts with the setup thread, before the keys load, so the OS lookups run beside
//! the keys' load and never on the window's start path. `Agents`, which starts after the keys,
//! is bound to it then ([`NetworkFollow::bind`]) and given the route resolved so far.
//!
//! The first route is awaited ([`NetworkFollow::ready`], at most until [`FIRST_ROUTE_WAIT`] after
//! the setup's start) before a command that reaches the network (an install, a retry, a sign-in,
//! first run's install) goes to `Agents`: behind a proxy set only in the OS, a first try that
//! went direct would be refused by the firm's firewall and counted as a failed try. The start's
//! `agent.keys_loading` launch refusal is lifted only after it too (`refusal::lift_at_start`), so
//! no agent launched at start goes direct for its whole life.
//!
//! A route is fresh for [`ROUTE_FRESH_FOR`] (60 s): past it the thread resolves the last read
//! again by itself, so a laptop that changed networks (office, home, VPN) gets the new proxy
//! within about a minute. Launches read the route from the slot and never wait (the host's actor
//! builds their env); each install try asks for a fresh one first ([`NetworkFollow::refresher`],
//! `InstallConfig::refresh`), waiting at most the budget, and within the 60 s takes the route as
//! it is, with no OS lookup.

use std::collections::BTreeMap;
use std::pin::pin;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_agents::{Clock, Network, SystemClock};
use clauseo_process::{AsyncIoTimer, Timer as _};
use futures::FutureExt as _;
use futures::channel::oneshot;
use futures::future::{Either, Shared, select};
use url::Url;

use super::{
    NetworkFacts, NetworkRoute, NetworkSlot, OsLookup, ProxyEnv, ask_all, os_targets, resolve,
};
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

/// How long a route is used before it is resolved again.
pub(crate) const ROUTE_FRESH_FOR: Duration = Duration::from_secs(60);

/// How long the lookups past the budget are waited for, on their own thread, before the route
/// is made again with their answers (the sandbox bounds each lookup at 10 s).
const LATE_WAIT: Duration = Duration::from_secs(15);

/// What the network thread is asked.
enum Ask {
    /// A settings read: the next route is made from it.
    Facts(NetworkFacts),
    /// A fresh route, if the last is stale; the network afterwards goes back on the sender.
    Refresh(mpsc::Sender<Network>),
    /// The answers of lookups that ended past the budget, with the resolve that asked for them
    /// (`Freshness::made` once it was noted): the route is made again with them unless another
    /// was made since, so a slow WPAD answer is not lost until the next change.
    Late(u64, Box<LateRoute>),
}

/// A route to make again once lookups past the budget have answered.
struct LateRoute {
    facts: NetworkFacts,
    env: ProxyEnv,
    /// Every answer, those in time and those late.
    answers: BTreeMap<Url, Option<String>>,
}

/// When the last route was made, and from what read.
struct Freshness {
    clock: Arc<dyn Clock>,
    fresh_for: Duration,
    /// The last read and when its route was made.
    last: Option<(NetworkFacts, Duration)>,
    /// How many routes have been made: late answers apply only to the resolve that asked.
    made: u64,
}

impl Freshness {
    /// Notes a route made from `facts` now.
    fn resolved(&mut self, facts: NetworkFacts) {
        self.last = Some((facts, self.clock.monotonic()));
        self.made += 1;
    }

    /// The last read, when its route is stale.
    fn stale(&self) -> Option<&NetworkFacts> {
        let (facts, at) = self.last.as_ref()?;
        (self.clock.monotonic().saturating_sub(*at) >= self.fresh_for).then_some(facts)
    }

    /// How long until the route is stale; `None` before the first.
    fn due_in(&self) -> Option<Duration> {
        let (_, at) = self.last.as_ref()?;
        Some((*at + self.fresh_for).saturating_sub(self.clock.monotonic()))
    }
}

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
    /// Asks for a fresh route and waits at most the given time for it ([`Self::refresher`]).
    refresh: Arc<dyn Fn(Duration) -> Option<Network> + Send + Sync>,
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
            refresh: Arc::new(|_wait| None),
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

    /// What an install try calls first (`InstallConfig::refresh`): the network of a fresh route,
    /// resolved again when the last is older than [`ROUTE_FRESH_FOR`] (at most the budget,
    /// [`FIRST_ROUTE_WAIT`]), else the last as it is; `None` when none came in time.
    pub(crate) fn refresher(&self) -> impl Fn() -> Option<Network> + Send + Sync + 'static {
        let refresh = Arc::clone(&self.refresh);
        move || refresh(FIRST_ROUTE_WAIT)
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
        let refresher = Arc::clone(&follower);
        Self {
            sink: Box::new(move |facts| lock(&taker).take(&facts)),
            first,
            started: SystemClock.monotonic(),
            follower: Some(follower),
            refresh: Arc::new(move |_wait| lock(&refresher).refresh_if_stale()),
        }
    }
}

/// What makes a route from a read: the environment and the OS, read at each one.
#[derive(Clone)]
struct Resolver {
    env: Arc<dyn Fn() -> ProxyEnv + Send + Sync>,
    os: Arc<OsLookup>,
    os_fallback: bool,
    /// [`OS_LOOKUP_BUDGET`], shorter in tests.
    budget: Duration,
}

impl Resolver {
    /// The route for `facts`, the OS asked about every target at once, within the budget.
    fn resolve(&self, facts: &NetworkFacts) -> NetworkRoute {
        self.resolve_late(facts, |_late| {})
    }

    /// [`Resolver::resolve`]; lookups still running past the budget are handed to `late` with
    /// what is needed to make the route again once they answer.
    fn resolve_late(
        &self,
        facts: &NetworkFacts,
        late: impl FnOnce(Box<dyn FnOnce() -> LateRoute + Send>),
    ) -> NetworkRoute {
        let env = (self.env)();
        let targets = os_targets(facts, &env, self.os_fallback);
        let answers = ask_all(targets, &self.os, self.budget);
        let answered = |target: &Url| answers.got.get(target).cloned().flatten();
        let route = resolve(facts, &env, self.os_fallback, &answered);
        if let Some(running) = answers.late {
            let (facts, mut got) = (facts.clone(), answers.got.clone());
            late(Box::new(move || {
                got.extend(running.collect(LATE_WAIT));
                LateRoute {
                    facts,
                    env,
                    answers: got,
                }
            }));
        }
        route
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
    /// When the last route was made ([`Follower::take`] on the caller's thread; the network
    /// thread keeps its own).
    fresh: Freshness,
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
                budget: OS_LOOKUP_BUDGET,
            },
            slot,
            agents: None,
            pushed: None,
            last: None,
            logged: None,
            landed: None,
            fresh: Freshness {
                clock: Arc::new(SystemClock),
                fresh_for: ROUTE_FRESH_FOR,
                last: None,
                made: 0,
            },
        }
    }

    /// With `budget` instead of [`OS_LOOKUP_BUDGET`] (tests).
    #[cfg(test)]
    pub(crate) fn lookup_budget(mut self, budget: Duration) -> Self {
        self.resolver.budget = budget;
        self
    }

    /// With `fresh_for` instead of [`ROUTE_FRESH_FOR`] (tests).
    #[cfg(test)]
    pub(crate) fn fresh_for(mut self, fresh_for: Duration) -> Self {
        self.fresh.fresh_for = fresh_for;
        self
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
        self.fresh.resolved(facts.clone());
        self.hand(route);
    }

    /// The last read resolved again when its route is stale; then the last network.
    #[cfg(test)]
    fn refresh_if_stale(&mut self) -> Option<Network> {
        if let Some(facts) = self.fresh.stale().cloned() {
            self.take(&facts);
        }
        self.last.clone()
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
    let fresh = Freshness {
        clock: Arc::clone(&follower.fresh.clock),
        fresh_for: follower.fresh.fresh_for,
        last: None,
        made: 0,
    };
    let (send, receive) = mpsc::channel::<Ask>();
    let follower = Arc::new(Mutex::new(follower));
    let on_thread = Arc::clone(&follower);
    let spawned = std::thread::Builder::new()
        .name("clauseo-setup-network".to_owned())
        .spawn({
            let late = send.clone();
            move || serve(&receive, &late, &resolver, fresh, &on_thread)
        });
    let (sink, refresh): (Box<dyn Fn(NetworkFacts) + Send + Sync>, Refresh) = match spawned {
        Ok(_thread) => {
            let asks = send.clone();
            let refresh: Refresh = Arc::new(move |wait| {
                let (reply, answer) = mpsc::channel();
                asks.send(Ask::Refresh(reply)).ok()?;
                answer.recv_timeout(wait).ok()
            });
            let sink = Box::new(move |facts| {
                // Gone only with the thread, which ends with the app.
                let _sent = send.send(Ask::Facts(facts));
            });
            (sink, refresh)
        }
        Err(err) => {
            tracing::warn!(%err, "no network thread: the proxy is resolved inline");
            let taker = Arc::clone(&follower);
            let last = Arc::clone(&follower);
            let refresh: Refresh = Arc::new(move |_wait| lock(&last).last.clone());
            (Box::new(move |facts| lock(&taker).take(&facts)), refresh)
        }
    };
    NetworkFollow {
        sink,
        first,
        started,
        follower: Some(follower),
        refresh,
    }
}

/// Waits, on a thread of its own, for the lookups past the budget, then asks the network thread
/// to make the route again with their answers.
fn watch_late(collect: Box<dyn FnOnce() -> LateRoute + Send>, made: u64, send: &mpsc::Sender<Ask>) {
    let send = send.clone();
    let spawned = std::thread::Builder::new()
        .name("clauseo-proxy-late".to_owned())
        .spawn(move || {
            // Gone only with the network thread.
            let _sent = send.send(Ask::Late(made, Box::new(collect())));
        });
    if let Err(err) = spawned {
        tracing::warn!(%err, "the late OS proxy answers are not waited for");
    }
}

/// What [`NetworkFollow::refresher`] calls: a fresh route within the given wait.
type Refresh = Arc<dyn Fn(Duration) -> Option<Network> + Send + Sync>;

/// The network thread's loop, until every sender is gone: each read (the newest of any queued)
/// is resolved, and the last again once stale, by itself or when a refresh asks; every refresh
/// gets the network afterwards. Resolved with no lock held, so a bind never waits on the OS.
fn serve(
    receive: &mpsc::Receiver<Ask>,
    send: &mpsc::Sender<Ask>,
    resolver: &Resolver,
    mut fresh: Freshness,
    follower: &Mutex<Follower>,
) {
    loop {
        let ask = match fresh.due_in() {
            Some(wait) => match receive.recv_timeout(wait) {
                Ok(ask) => Some(ask),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => return,
            },
            None => match receive.recv() {
                Ok(ask) => Some(ask),
                Err(_) => return,
            },
        };
        let mut newest = None;
        let mut replies = Vec::new();
        let mut late = None;
        for ask in ask
            .into_iter()
            .chain(std::iter::from_fn(|| receive.try_recv().ok()))
        {
            match ask {
                Ask::Facts(facts) => newest = Some(facts),
                Ask::Refresh(reply) => replies.push(reply),
                Ask::Late(made, route) => late = Some((made, route)),
            }
        }
        if let Some(facts) = newest.or_else(|| fresh.stale().cloned()) {
            let made = fresh.made + 1;
            let route = resolver.resolve_late(&facts, |collect| watch_late(collect, made, send));
            fresh.resolved(facts);
            lock(follower).hand(route);
        } else if let Some((_, late)) = late.filter(|(made, _)| *made == fresh.made) {
            // No route made since they were asked: the route again with their answers, no lookup.
            let answered = |target: &Url| late.answers.get(target).cloned().flatten();
            let route = resolve(&late.facts, &late.env, resolver.os_fallback, &answered);
            lock(follower).hand(route);
        }
        let network = lock(follower).last.clone();
        for reply in replies {
            if let Some(network) = network.clone() {
                // The caller may have stopped waiting.
                let _sent = reply.send(network);
            }
        }
    }
}

#[cfg(test)]
mod tests;
