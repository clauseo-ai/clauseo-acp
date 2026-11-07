//! Where the egress proxy sends a connection it allows: straight to the host, or through the
//! firm's own HTTP proxy with `CONNECT`.
//!
//! First match wins: the firm's `network.upstream_proxy` ([`Upstream::Fixed`]); then, for
//! [`Upstream::System`], the host's `HTTPS_PROXY` or `https_proxy` for a CONNECT, or
//! `HTTP_PROXY` or `http_proxy` for a plain `http://` request (read once, when the [`Lookup`]
//! is made), with `NO_PROXY` or `no_proxy`; then the OS, asked about an `https://` or
//! `http://` URL to match (so a PAC script that branches on the scheme, and macOS's separate
//! HTTP proxy setting, apply): `CFNetwork` on macOS (it runs PAC), `WinHTTP` on Windows (PAC
//! and WPAD, the whole call bounded to 10 s), nothing on Linux. An OS answer is kept 60 s per
//! scheme, host and port, at most 1024 of them (the oldest goes first); at most 8 lookups run at
//! once, and a miss past them waits up to 2 s for a slot, then fails, uncached, with one
//! warning per process that names no host. A failed OS lookup (an API error, a PAC script that
//! throws or takes over 5 s) is cached apart from `DIRECT`, with whether the settings show the firm
//! meant a proxy ([`Failure`]), and logged by port only. [`upstream_for`] takes it as direct; the
//! egress proxy reuses the host's last good route, refuses, or goes direct (decision record 0039).
//!
//! No credentials, ever: a [`Route`] carries none, and a user name or password in `HTTPS_PROXY`
//! is dropped with a warning that never prints it. Only `http://` proxies: both an HTTP and an
//! HTTPS proxy of the OS settings are plain HTTP proxies that take `CONNECT`; an `https://`
//! proxy URL (TLS to the proxy) is refused.

mod answer;
#[cfg(any(windows, test))]
mod bounded;
#[cfg(target_os = "macos")]
mod macos;
mod os;
mod parse;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use url::Url;

#[cfg(test)]
use self::bounded::bounded;
use crate::{Clock, Error, SystemClock};
#[cfg(any(target_os = "macos", windows, test))]
pub(crate) use answer::Configured;
pub(crate) use answer::{Answer, Decided, Failure};
pub use answer::{UpstreamAnswer, upstream_answer, upstream_for};
use os::{os_intent, os_route};
#[cfg(test)]
use os::{settle, target_url};
use parse::{NoProxy, normal_host};

/// How long an OS answer is kept for one host and port: a proxy answer, a `DIRECT` and a failed
/// lookup (kept apart from `DIRECT`) alike, so a WPAD or PAC timeout stalls a connection at most
/// once a minute per host. Short enough that a laptop moved to another network (office, home, VPN)
/// gets the new answer within a minute.
const CACHE_FOR: Duration = Duration::from_secs(60);

/// The most OS answers kept. At the cap a new answer first drops the expired ones, then the
/// oldest, so random names under a firm wildcard cannot grow the cache.
const CACHE_MAX: usize = 1024;

/// The most OS lookups running at once per [`Lookup`]. Desktop's proxy resolver (S17) asks
/// about 5 hosts at once (the npm registry or its mirror, each agent's sign-in host and API
/// host), which 8 holds with room for the egress proxy's own misses beside them. A miss past it
/// waits up to [`OS_SLOT_WAIT`] for a slot, then fails, uncached.
const OS_LOOKUPS_MOST: usize = 8;

/// How long a miss waits for a free lookup slot before it fails.
const OS_SLOT_WAIT: Duration = if cfg!(test) {
    Duration::from_millis(200)
} else {
    Duration::from_secs(2)
};

/// Whether the "lookups all busy" warning was logged: once per process.
static BUSY_LOGGED: AtomicBool = AtomicBool::new(false);

/// Where the firm's upstream proxy comes from.
#[derive(Clone)]
pub enum Upstream {
    /// Always straight to the host.
    Direct,
    /// The firm's `network.upstream_proxy`: `http://host[:port]`, for every host.
    Fixed(Url),
    /// The host's `HTTPS_PROXY` (or `https_proxy`) with `NO_PROXY`, else the OS settings or PAC.
    System,
}

/// Shows only a fixed URL's scheme, host and port: never a password, path or query.
impl fmt::Debug for Upstream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Upstream::Direct => f.write_str("Direct"),
            Upstream::Fixed(url) => f
                .debug_tuple("Fixed")
                .field(&format_args!("{}", scheme_host_port(url)))
                .finish(),
            Upstream::System => f.write_str("System"),
        }
    }
}

/// `scheme://host:port` of `url`, with nothing else from it.
fn scheme_host_port(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    match url.port_or_known_default() {
        Some(port) => format!("{}://{host}:{port}", url.scheme()),
        None => format!("{}://{host}", url.scheme()),
    }
}

/// Which kind of request a route is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Scheme {
    /// A CONNECT, or the host's own HTTPS: `HTTPS_PROXY`, and the OS asked about `https://`.
    Https,
    /// A plain `http://` request: `HTTP_PROXY`, and the OS asked about `http://`.
    Http,
}

impl Scheme {
    fn of(url: &Url) -> Scheme {
        if url.scheme() == "http" {
            Scheme::Http
        } else {
            Scheme::Https
        }
    }
}

/// Where one connection goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Straight to the host.
    Direct,
    /// Through an HTTP proxy, with `CONNECT`. `host` is a name or a bare IP literal (an IPv6
    /// address without brackets).
    Http {
        /// The proxy's host.
        host: String,
        /// The proxy's port.
        port: u16,
    },
}

/// The host's proxy variables, as [`Lookup`] reads them.
#[derive(Debug, Clone, Default)]
pub(crate) struct Env {
    /// `HTTPS_PROXY`, else `https_proxy`; `None` when unset or empty.
    pub(crate) https: Option<String>,
    /// `HTTP_PROXY`, else `http_proxy`; `None` when unset or empty.
    pub(crate) http: Option<String>,
    /// `NO_PROXY`, else `no_proxy`; `None` when unset or empty.
    pub(crate) no_proxy: Option<String>,
}

impl Env {
    /// This process's variables. The upper-case name wins, as curl reads them.
    pub(crate) fn from_process() -> Env {
        let read = |names: [&str; 2]| {
            names.iter().find_map(|name| {
                std::env::var_os(name)
                    .and_then(|value| value.into_string().ok())
                    .filter(|value| !value.trim().is_empty())
            })
        };
        Env {
            https: read(["HTTPS_PROXY", "https_proxy"]),
            http: read(["HTTP_PROXY", "http_proxy"]),
            no_proxy: read(["NO_PROXY", "no_proxy"]),
        }
    }
}

/// The OS lookup of one scheme, host and port: blocking, may run PAC.
#[cfg(test)]
pub(crate) type OsRoute = dyn Fn(Scheme, &str, u16) -> Route + Send + Sync;

/// The OS lookup, which may fail: what [`OsLookup`] asks and caches.
pub(crate) type OsAnswer = dyn Fn(Scheme, &str, u16) -> Answer + Send + Sync;

/// Whether the settings show the firm meant a proxy, read when a lookup failed without asking
/// the OS for a route (no lookup slot, or too slow for the egress proxy).
type Intent = dyn Fn() -> Failure + Send + Sync;

/// One launch's upstream lookup, shared by its connections.
pub(crate) struct Lookup {
    source: Source,
}

enum Source {
    Direct,
    Fixed(Route),
    System(Box<System>),
}

/// `Upstream::System`: per scheme, the environment's proxy (with `NO_PROXY`) when usable, else
/// the OS.
struct System {
    https: Option<Route>,
    http: Option<Route>,
    no_proxy: NoProxy,
    os: OsLookup,
}

/// The OS lookup with its cache.
struct OsLookup {
    resolve: Box<OsAnswer>,
    intent: Box<Intent>,
    clock: Box<dyn Clock>,
    /// Per scheme, normalised host and port: the answer and when it was got.
    cache: Mutex<Cache>,
    /// The lookups running now, one per key; `landed` wakes those waiting on one.
    flights: Mutex<Flights>,
    landed: Condvar,
}

/// A scheme, normalised host and port.
type Key = (Scheme, String, u16);

/// OS answers per key, with when each was got (a failure too, kept apart from `DIRECT`).
type Cache = HashMap<Key, (Answer, Duration)>;

/// The OS lookups in flight.
#[derive(Default)]
struct Flights {
    running: HashSet<Key>,
    /// Callers waiting for another's lookup of the same key (read by tests).
    waiting: usize,
}

/// One running lookup: ends its flight and wakes the waiters when it drops, a panic included.
struct Flight<'a> {
    os: &'a OsLookup,
    key: &'a Key,
}

impl Drop for Flight<'_> {
    fn drop(&mut self) {
        self.os.flights().running.remove(self.key);
        self.os.landed.notify_all();
    }
}

impl Lookup {
    /// Refuses a Fixed URL that is not `http://host[:port]` with no userinfo, path, query or
    /// fragment (`Error::ProxyConfig`).
    pub(crate) fn new(upstream: Upstream) -> Result<Lookup, Error> {
        Ok(
            Lookup::with_parts(upstream, Env::from_process(), os_route, SystemClock::new())?
                .with_intent(os_intent),
        )
    }

    /// [`Lookup::new`] with the environment, the OS lookup (a `Route`, or an
    /// [`UpstreamAnswer`] that may fail) and the clock given.
    pub(crate) fn with_parts<A: Into<Answer>>(
        upstream: Upstream,
        env: Env,
        resolve: impl Fn(Scheme, &str, u16) -> A + Send + Sync + 'static,
        clock: impl Clock + 'static,
    ) -> Result<Lookup, Error> {
        let source = match upstream {
            Upstream::Direct => Source::Direct,
            Upstream::Fixed(url) => Source::Fixed(parse::fixed_proxy(&url).map_err(|bad| {
                Error::ProxyConfig(format!(
                    "network.upstream_proxy must be http://host[:port]: {}",
                    bad.reason()
                ))
            })?),
            Upstream::System => Source::System(Box::new(System::new(
                env,
                Box::new(move |scheme: Scheme, host: &str, port: u16| -> Answer {
                    resolve(scheme, host, port).into()
                }),
                Box::new(clock),
            ))),
        };
        Ok(Lookup { source })
    }

    /// Reads the settings with `intent` when a lookup fails without asking the OS (by default,
    /// from [`Lookup::with_parts`], nothing shows a proxy meant).
    pub(crate) fn with_intent(
        mut self,
        intent: impl Fn() -> Failure + Send + Sync + 'static,
    ) -> Self {
        if let Source::System(system) = &mut self.source {
            system.os.intent = Box::new(intent);
        }
        self
    }

    /// Blocking (the OS may run PAC). A failed OS lookup goes direct; the egress proxy asks
    /// [`Lookup::decide`] instead.
    #[cfg(test)]
    pub(crate) fn route(&self, scheme: Scheme, host: &str, port: u16) -> Route {
        self.answer(scheme, host, port).route()
    }

    /// [`Lookup::route`], a failed OS lookup kept apart from `DIRECT`.
    pub(crate) fn answer(&self, scheme: Scheme, host: &str, port: u16) -> UpstreamAnswer {
        self.decide(scheme, host, port).answer.into()
    }

    /// [`Lookup::answer`] with a failure's [`Failure`] and the route's age: the egress proxy's.
    pub(crate) fn decide(&self, scheme: Scheme, host: &str, port: u16) -> Decided {
        match &self.source {
            Source::Direct => Decided::plain(Route::Direct),
            Source::Fixed(route) => Decided::plain(route.clone()),
            Source::System(system) => system.route(scheme, host, port),
        }
    }

    /// Blocking: whether the settings show a proxy meant, for a lookup too slow to wait for.
    pub(crate) fn intent(&self) -> Failure {
        match &self.source {
            Source::System(system) => (system.os.intent)(),
            Source::Direct | Source::Fixed(_) => Failure::Unmeant,
        }
    }

    /// The lookup clock's time (zero when the lookup has no OS part).
    pub(crate) fn now(&self) -> Duration {
        match &self.source {
            Source::System(system) => system.os.clock.now(),
            Source::Direct | Source::Fixed(_) => Duration::ZERO,
        }
    }

    /// How many OS answers are cached (0 when the lookup has no OS part).
    #[cfg(test)]
    fn cached(&self) -> usize {
        match &self.source {
            Source::System(system) => system.os.cached(),
            Source::Direct | Source::Fixed(_) => 0,
        }
    }

    /// How many callers wait for another's OS lookup (0 when the lookup has no OS part).
    #[cfg(test)]
    fn waiting(&self) -> usize {
        match &self.source {
            Source::System(system) => system.os.flights().waiting,
            Source::Direct | Source::Fixed(_) => 0,
        }
    }
}

impl System {
    fn new(env: Env, resolve: Box<OsAnswer>, clock: Box<dyn Clock>) -> System {
        let Env {
            https,
            http,
            no_proxy,
        } = env;
        System {
            https: env_route("HTTPS_PROXY", https.as_deref()),
            http: env_route("HTTP_PROXY", http.as_deref()),
            no_proxy: NoProxy::parse(no_proxy.as_deref().unwrap_or_default()),
            os: OsLookup {
                resolve,
                intent: Box::new(Failure::default),
                clock,
                cache: Mutex::new(HashMap::new()),
                flights: Mutex::new(Flights::default()),
                landed: Condvar::new(),
            },
        }
    }

    fn route(&self, scheme: Scheme, host: &str, port: u16) -> Decided {
        let host = normal_host(host);
        let from_env = match scheme {
            Scheme::Https => &self.https,
            Scheme::Http => &self.http,
        };
        match from_env {
            Some(_) if self.no_proxy.matches(&host, port) => Decided::plain(Route::Direct),
            Some(route) => Decided::plain(route.clone()),
            None => {
                let (answer, got) = self.os.route(scheme, &host, port);
                let got = matches!(answer, Answer::Route(_)).then_some(got);
                let now = self.os.clock.now();
                Decided { answer, got, now }
            }
        }
    }
}

/// One proxy variable's route when it is usable; `None` (so the OS answers) when it is unset
/// or unusable, with a warning that names the variable and never its value.
fn env_route(name: &str, raw: Option<&str>) -> Option<Route> {
    match parse::env_proxy(raw?) {
        Ok(proxy) => {
            if proxy.dropped_userinfo {
                tracing::warn!(
                    "{name} has a user name or password; Clauseo dropped it and uses the proxy \
                     without"
                );
            }
            Some(proxy.route)
        }
        Err(bad) => {
            tracing::warn!(
                "{name} is ignored, and the OS proxy settings are used: {}",
                bad.reason()
            );
            None
        }
    }
}

impl OsLookup {
    /// A fresh cached answer, else the OS's, with no lock held while the OS answers. One lookup
    /// runs per key: a second caller waits for the first's answer. At most [`OS_LOOKUPS_MOST`]
    /// run at once; a miss past that waits up to [`OS_SLOT_WAIT`] for a slot, then fails for this
    /// request (its [`Failure`] read from the settings), uncached, and the first time in the
    /// process says so in the log. Returns the answer with when it was got (now, for a failure
    /// past the slots).
    fn route(&self, scheme: Scheme, host: &str, port: u16) -> (Answer, Duration) {
        let key = (scheme, host.to_owned(), port);
        let mut flights = self.flights();
        let mut slot_waited = false;
        loop {
            if let Some(route) = self.fresh(&key) {
                return route;
            }
            if flights.running.contains(&key) {
                flights.waiting += 1;
                flights = self
                    .landed
                    .wait_while(flights, |f| f.running.contains(&key))
                    .unwrap_or_else(PoisonError::into_inner);
                flights.waiting -= 1;
                continue;
            }
            if flights.running.len() < OS_LOOKUPS_MOST {
                break;
            }
            if slot_waited {
                if !BUSY_LOGGED.swap(true, Ordering::Relaxed) {
                    tracing::warn!("the OS proxy lookups were all busy; a lookup failed");
                }
                drop(flights);
                return (Answer::Failed((self.intent)()), self.clock.now());
            }
            slot_waited = true;
            let busy =
                |f: &mut Flights| f.running.len() >= OS_LOOKUPS_MOST && !f.running.contains(&key);
            flights = self
                .landed
                .wait_timeout_while(flights, OS_SLOT_WAIT, busy)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        flights.running.insert(key.clone());
        drop(flights);
        let _flight = Flight {
            os: self,
            key: &key,
        };
        let route = (self.resolve)(scheme, host, port);
        let at = self.store(key.clone(), route.clone());
        (route, at)
    }

    fn flights(&self) -> MutexGuard<'_, Flights> {
        self.flights.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// The cached answer for `key`, with when it was got, if it is under [`CACHE_FOR`] old.
    fn fresh(&self, key: &Key) -> Option<(Answer, Duration)> {
        let now = self.clock.now();
        let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        cache
            .get(key)
            .filter(|(_, at)| now.saturating_sub(*at) < CACHE_FOR)
            .cloned()
    }

    /// Keeps an answer, within [`CACHE_MAX`]; returns when it was got.
    fn store(&self, key: Key, route: Answer) -> Duration {
        let at = self.clock.now();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= CACHE_MAX && !cache.contains_key(&key) {
            cache.retain(|_, (_, got)| at.saturating_sub(*got) < CACHE_FOR);
            while cache.len() >= CACHE_MAX {
                let oldest = cache
                    .iter()
                    .min_by_key(|(_, (_, got))| *got)
                    .map(|(key, _)| key.clone());
                let Some(oldest) = oldest else { break };
                cache.remove(&oldest);
            }
        }
        cache.insert(key, (route, at));
        at
    }

    #[cfg(test)]
    fn cached(&self) -> usize {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}
