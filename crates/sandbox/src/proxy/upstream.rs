//! Where the egress proxy sends a connection it allows: straight to the host, or through the
//! firm's own HTTP proxy with `CONNECT`.
//!
//! First match wins: the firm's `network.upstream_proxy` ([`Upstream::Fixed`]); then, for
//! [`Upstream::System`], the host's `HTTPS_PROXY` or `https_proxy` for a CONNECT, or
//! `HTTP_PROXY` or `http_proxy` for a plain `http://` request (read once, when the [`Lookup`]
//! is made), with `NO_PROXY` or `no_proxy`; then the OS, asked about an `https://` or
//! `http://` URL to match (so a PAC script that branches on the scheme, and macOS's separate
//! HTTP proxy setting, apply): `CFNetwork` on macOS (it runs PAC), `WinHTTP` on Windows (PAC
//! and WPAD, the whole call bounded to 10 s), nothing on Linux. An OS answer is kept 5 minutes
//! per scheme, host and port, at most 1024 of them (the oldest goes first). A failed OS lookup (an API error, a PAC script that throws or takes over 5 s)
//! goes direct and is logged with the host only: the firm's network refuses a direct connection
//! it does not allow, and the agent sees that refusal rather than a hang.
//!
//! No credentials, ever: a [`Route`] carries none, and a user name or password in `HTTPS_PROXY`
//! is dropped with a warning that never prints it. Only `http://` proxies: both an HTTP and an
//! HTTPS proxy of the OS settings are plain HTTP proxies that take `CONNECT`; an `https://`
//! proxy URL (TLS to the proxy) is refused.

#[cfg(target_os = "macos")]
mod macos;
mod parse;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

use std::collections::HashMap;
use std::fmt;
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use url::Url;

use crate::{Clock, Error, SystemClock};
use parse::{NoProxy, normal_host};

/// How long an OS answer is kept for one host and port.
const CACHE_FOR: Duration = Duration::from_mins(5);

/// The most OS answers kept. At the cap a new answer first drops the expired ones, then the
/// oldest, so random names under a firm wildcard cannot grow the cache.
const CACHE_MAX: usize = 1024;

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
#[expect(
    clippy::struct_field_names,
    reason = "each field is named after the variable it holds"
)]
pub(crate) struct Env {
    /// `HTTPS_PROXY`, else `https_proxy`; `None` when unset or empty.
    pub(crate) https_proxy: Option<String>,
    /// `HTTP_PROXY`, else `http_proxy`; `None` when unset or empty.
    pub(crate) http_proxy: Option<String>,
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
            https_proxy: read(["HTTPS_PROXY", "https_proxy"]),
            http_proxy: read(["HTTP_PROXY", "http_proxy"]),
            no_proxy: read(["NO_PROXY", "no_proxy"]),
        }
    }
}

/// The OS lookup of one scheme, host and port: blocking, may run PAC.
pub(crate) type OsRoute = dyn Fn(Scheme, &str, u16) -> Route + Send + Sync;

/// One launch's upstream lookup, shared by its connections.
pub(crate) struct Lookup {
    source: Source,
}

enum Source {
    Direct,
    Fixed(Route),
    System(System),
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
    resolve: Box<OsRoute>,
    clock: Box<dyn Clock>,
    /// Per scheme, normalised host and port: the answer and when it was got.
    cache: Mutex<Cache>,
}

/// OS answers per scheme, normalised host and port, with when each was got.
type Cache = HashMap<(Scheme, String, u16), (Route, Duration)>;

impl Lookup {
    /// Refuses a Fixed URL that is not `http://host[:port]` with no userinfo, path, query or
    /// fragment (`Error::ProxyConfig`).
    pub(crate) fn new(upstream: Upstream) -> Result<Lookup, Error> {
        Lookup::with_parts(upstream, Env::from_process(), os_route, SystemClock::new())
    }

    /// [`Lookup::new`] with the environment, the OS lookup and the clock given.
    pub(crate) fn with_parts(
        upstream: Upstream,
        env: Env,
        resolve: impl Fn(Scheme, &str, u16) -> Route + Send + Sync + 'static,
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
            Upstream::System => {
                Source::System(System::new(env, Box::new(resolve), Box::new(clock)))
            }
        };
        Ok(Lookup { source })
    }

    /// Blocking (the OS may run PAC): the proxy calls it through `blocking::unblock`.
    pub(crate) fn route(&self, scheme: Scheme, host: &str, port: u16) -> Route {
        match &self.source {
            Source::Direct => Route::Direct,
            Source::Fixed(route) => route.clone(),
            Source::System(system) => system.route(scheme, host, port),
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
}

impl System {
    fn new(env: Env, resolve: Box<OsRoute>, clock: Box<dyn Clock>) -> System {
        let Env {
            https_proxy,
            http_proxy,
            no_proxy,
        } = env;
        System {
            https: env_route("HTTPS_PROXY", https_proxy.as_deref()),
            http: env_route("HTTP_PROXY", http_proxy.as_deref()),
            no_proxy: NoProxy::parse(no_proxy.as_deref().unwrap_or_default()),
            os: OsLookup {
                resolve,
                clock,
                cache: Mutex::new(HashMap::new()),
            },
        }
    }

    fn route(&self, scheme: Scheme, host: &str, port: u16) -> Route {
        let host = normal_host(host);
        let from_env = match scheme {
            Scheme::Https => &self.https,
            Scheme::Http => &self.http,
        };
        match from_env {
            Some(_) if self.no_proxy.matches(&host, port) => Route::Direct,
            Some(route) => route.clone(),
            None => self.os.route(scheme, &host, port),
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
    /// A fresh cached answer, else the OS's, with no lock held while the OS answers.
    fn route(&self, scheme: Scheme, host: &str, port: u16) -> Route {
        let key = (scheme, host.to_owned(), port);
        let now = self.clock.now();
        {
            let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((route, at)) = cache.get(&key)
                && now.saturating_sub(*at) < CACHE_FOR
            {
                return route.clone();
            }
        }
        let route = (self.resolve)(scheme, host, port);
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
        cache.insert(key, (route.clone(), at));
        route
    }

    #[cfg(test)]
    fn cached(&self) -> usize {
        self.cache
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }
}

/// The upstream for the host's own HTTPS (or HTTP) outside the jail (stage E's connector relay).
/// Blocking, like [`Lookup::route`]. [`Upstream::System`] shares one process-wide [`Lookup`]
/// and its cache, its environment read at the first call. A [`Upstream::Fixed`] URL that
/// [`start_proxy`](crate::start_proxy) would refuse is refused here too
/// (`Error::ProxyConfig`), so the connection fails where the lawyer sees it rather than going
/// around the proxy the firm requires.
pub fn upstream_for(upstream: &Upstream, url: &Url) -> Result<Route, Error> {
    static SYSTEM: OnceLock<Lookup> = OnceLock::new();
    let made;
    let lookup = if let Upstream::System = upstream {
        SYSTEM.get_or_init(|| Lookup {
            source: Source::System(System::new(
                Env::from_process(),
                Box::new(os_route),
                Box::new(SystemClock::new()),
            )),
        })
    } else {
        made = Lookup::new(upstream.clone())?;
        &made
    };
    let Some(host) = url.host_str() else {
        return Ok(Route::Direct);
    };
    let scheme = Scheme::of(url);
    let port = url.port_or_known_default().unwrap_or(match scheme {
        Scheme::Https => 443,
        Scheme::Http => 80,
    });
    Ok(lookup.route(scheme, host, port))
}

/// The `https://host:port/` or `http://host:port/` the OS is asked about, or `None` for a host
/// no URL can hold.
#[cfg(any(target_os = "macos", windows, test))]
fn target_url(scheme: Scheme, host: &str, port: u16) -> Option<String> {
    let authority = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let scheme = match scheme {
        Scheme::Https => "https",
        Scheme::Http => "http",
    };
    let url = Url::parse(&format!("{scheme}://{authority}:{port}/")).ok()?;
    url.host_str().is_some().then(|| url.into())
}

/// An OS answer, or direct when the lookup failed, logged by host and port only.
#[cfg(any(target_os = "macos", windows, test))]
fn settle(host: &str, port: u16, answer: Result<Route, String>) -> Route {
    answer.unwrap_or_else(|why| {
        tracing::warn!(host, port, why, "the OS proxy lookup failed; going direct");
        Route::Direct
    })
}

/// The OS lookup: `CFNetwork`'s proxies for the target URL.
#[cfg(target_os = "macos")]
fn os_route(scheme: Scheme, host: &str, port: u16) -> Route {
    let answer = target_url(scheme, host, port)
        .ok_or_else(|| "the host is not a URL host".to_owned())
        .and_then(|target| macos::route(&target));
    settle(host, port, answer)
}

/// The OS lookup: `WinHTTP`'s proxy for the target URL, the whole call bounded to
/// [`WINDOWS_LOOKUP_LIMIT`]: WPAD's DHCP and DNS discovery do not all honour `WinHTTP`'s
/// timeouts.
#[cfg(windows)]
fn os_route(scheme: Scheme, host: &str, port: u16) -> Route {
    let answer = target_url(scheme, host, port)
        .ok_or_else(|| "the host is not a URL host".to_owned())
        .and_then(|target| {
            let host = host.to_owned();
            bounded(WINDOWS_LOOKUP_LIMIT, move || {
                windows::route(&target, &host, port)
            })
        });
    settle(host, port, answer)
}

/// The longest a Windows lookup (WPAD discovery and the PAC run included) may take.
#[cfg(windows)]
const WINDOWS_LOOKUP_LIMIT: Duration = Duration::from_secs(10);

/// Runs `lookup` on its own thread and waits at most `limit` for it. A lookup still running
/// then is left to finish on its thread, and its answer is dropped.
#[cfg(windows)]
fn bounded(
    limit: Duration,
    lookup: impl FnOnce() -> Result<Route, String> + Send + 'static,
) -> Result<Route, String> {
    let (answer, answered) = std::sync::mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("clauseo-proxy-lookup".to_owned())
        .spawn(move || {
            // The waiter may have given up; its answer is then dropped.
            drop(answer.send(lookup()));
        })
        .map_err(|error| format!("could not start the lookup thread ({error})"))?;
    answered
        .recv_timeout(limit)
        .map_err(|_timeout| format!("the lookup took over {} s", limit.as_secs()))?
}

/// The OS lookup on Linux and others: none, so direct.
#[cfg(not(any(target_os = "macos", windows)))]
fn os_route(_scheme: Scheme, _host: &str, _port: u16) -> Route {
    Route::Direct
}
