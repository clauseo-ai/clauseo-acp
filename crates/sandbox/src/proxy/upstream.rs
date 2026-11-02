//! Where the egress proxy sends a connection it allows: straight to the host, or through the
//! firm's own HTTP proxy with `CONNECT`.
//!
//! First match wins: the firm's `network.upstream_proxy` ([`Upstream::Fixed`]); then, for
//! [`Upstream::System`], the host's `HTTPS_PROXY` or `https_proxy` (read once, when the
//! [`Lookup`] is made) with `NO_PROXY` or `no_proxy`; then the OS: `CFNetwork` on macOS (it runs
//! PAC), `WinHTTP` on Windows (PAC and WPAD), nothing on Linux. An OS answer is kept 5 minutes per
//! host and port. A failed OS lookup (an API error, a PAC script that throws or takes over 5 s)
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
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::Duration;

use url::Url;

use crate::{Clock, Error, SystemClock};
use parse::{NoProxy, normal_host};

/// How long an OS answer is kept for one host and port.
const CACHE_FOR: Duration = Duration::from_mins(5);

/// Above this many cached answers, a new answer first drops the expired ones.
const CACHE_PRUNE_AT: usize = 512;

/// Where the firm's upstream proxy comes from.
#[derive(Debug, Clone)]
pub enum Upstream {
    /// Always straight to the host.
    Direct,
    /// The firm's `network.upstream_proxy`: `http://host[:port]`, for every host.
    Fixed(Url),
    /// The host's `HTTPS_PROXY` (or `https_proxy`) with `NO_PROXY`, else the OS settings or PAC.
    System,
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
    pub(crate) https_proxy: Option<String>,
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
            no_proxy: read(["NO_PROXY", "no_proxy"]),
        }
    }
}

/// The OS lookup of one host and port: blocking, may run PAC.
type OsRoute = dyn Fn(&str, u16) -> Route + Send + Sync;

/// One launch's upstream lookup, shared by its connections.
pub(crate) struct Lookup {
    source: Source,
}

enum Source {
    Direct,
    Fixed(Route),
    /// `HTTPS_PROXY`, with `NO_PROXY`.
    Env {
        route: Route,
        no_proxy: NoProxy,
    },
    Os(OsLookup),
}

/// The OS lookup with its cache.
struct OsLookup {
    resolve: Box<OsRoute>,
    clock: Box<dyn Clock>,
    /// Per normalised host and port: the answer and when it was got.
    cache: Mutex<HashMap<(String, u16), (Route, Duration)>>,
}

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
        resolve: impl Fn(&str, u16) -> Route + Send + Sync + 'static,
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
            Upstream::System => system_source(env, Box::new(resolve), Box::new(clock)),
        };
        Ok(Lookup { source })
    }

    /// Blocking (the OS may run PAC): the proxy calls it through `blocking::unblock`.
    pub(crate) fn route(&self, host: &str, port: u16) -> Route {
        match &self.source {
            Source::Direct => Route::Direct,
            Source::Fixed(route) => route.clone(),
            Source::Env { route, no_proxy } => {
                if no_proxy.matches(&normal_host(host), port) {
                    Route::Direct
                } else {
                    route.clone()
                }
            }
            Source::Os(os) => os.route(&normal_host(host), port),
        }
    }
}

/// `HTTPS_PROXY` when it is usable, else the OS.
fn system_source(env: Env, resolve: Box<OsRoute>, clock: Box<dyn Clock>) -> Source {
    if let Some(raw) = env.https_proxy {
        match parse::env_proxy(&raw) {
            Ok(proxy) => {
                if proxy.dropped_userinfo {
                    tracing::warn!(
                        "HTTPS_PROXY has a user name or password; Clauseo dropped it and uses \
                         the proxy without"
                    );
                }
                return Source::Env {
                    route: proxy.route,
                    no_proxy: NoProxy::parse(env.no_proxy.as_deref().unwrap_or_default()),
                };
            }
            Err(bad) => {
                tracing::warn!(
                    "HTTPS_PROXY is ignored, and the OS proxy settings are used: {}",
                    bad.reason()
                );
            }
        }
    }
    Source::Os(OsLookup {
        resolve,
        clock,
        cache: Mutex::new(HashMap::new()),
    })
}

impl OsLookup {
    /// A fresh cached answer, else the OS's, with no lock held while the OS answers.
    fn route(&self, host: &str, port: u16) -> Route {
        let key = (host.to_owned(), port);
        let now = self.clock.now();
        {
            let cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((route, at)) = cache.get(&key)
                && now.saturating_sub(*at) < CACHE_FOR
            {
                return route.clone();
            }
        }
        let route = (self.resolve)(host, port);
        let at = self.clock.now();
        let mut cache = self.cache.lock().unwrap_or_else(PoisonError::into_inner);
        if cache.len() >= CACHE_PRUNE_AT {
            cache.retain(|_, (_, got)| at.saturating_sub(*got) < CACHE_FOR);
        }
        cache.insert(key, (route.clone(), at));
        route
    }
}

/// The upstream for the host's own HTTPS outside the jail (stage E's connector relay). Blocking,
/// like [`Lookup::route`]. [`Upstream::System`] shares one process-wide [`Lookup`] and its
/// cache, its environment read at the first call. A [`Upstream::Fixed`] URL that
/// [`Lookup::new`] would refuse goes direct, with a warning.
pub fn upstream_for(upstream: &Upstream, url: &Url) -> Route {
    static SYSTEM: OnceLock<Lookup> = OnceLock::new();
    let Some(host) = url.host_str() else {
        return Route::Direct;
    };
    let port = url.port_or_known_default().unwrap_or(443);
    let made;
    let lookup = if let Upstream::System = upstream {
        SYSTEM.get_or_init(|| Lookup::new(Upstream::System).unwrap_or_else(|error| refused(&error)))
    } else {
        made = Lookup::new(upstream.clone()).unwrap_or_else(|error| refused(&error));
        &made
    };
    lookup.route(host, port)
}

/// The direct lookup that stands in for a refused setting, with a warning (the reason only).
fn refused(error: &Error) -> Lookup {
    tracing::warn!("{error}; the connection goes direct");
    Lookup {
        source: Source::Direct,
    }
}

/// The `https://host:port/` the OS is asked about, or `None` for a host no URL can hold.
#[cfg(any(target_os = "macos", windows, test))]
fn target_url(host: &str, port: u16) -> Option<String> {
    let authority = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let url = Url::parse(&format!("https://{authority}:{port}/")).ok()?;
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

/// The OS lookup: `CFNetwork`'s proxies for `https://host:port/`.
#[cfg(target_os = "macos")]
fn os_route(host: &str, port: u16) -> Route {
    let answer = target_url(host, port)
        .ok_or_else(|| "the host is not a URL host".to_owned())
        .and_then(|target| macos::route(&target));
    settle(host, port, answer)
}

/// The OS lookup: `WinHTTP`'s proxy for `https://host:port/`.
#[cfg(windows)]
fn os_route(host: &str, port: u16) -> Route {
    let answer = target_url(host, port)
        .ok_or_else(|| "the host is not a URL host".to_owned())
        .and_then(|target| windows::route(&target, host, port));
    settle(host, port, answer)
}

/// The OS lookup on Linux and others: none, so direct.
#[cfg(not(any(target_os = "macos", windows)))]
fn os_route(_host: &str, _port: u16) -> Route {
    Route::Direct
}
