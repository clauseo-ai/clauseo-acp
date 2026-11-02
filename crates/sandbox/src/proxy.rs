//! The egress proxy: one per agent launch, on two loopback listeners, the jailed agent's only way
//! out and the browser's way out in every tier.
//!
//! It allows a CONNECT to port 443 of a host on the launch's allowlist and a plain `http://`
//! request to port 80 of one, chains to the firm's proxy when there is one, never looks inside
//! TLS (it holds no CA), refuses loopback and private addresses, and turns each refusal into a
//! [`Denial`](crate::Denial) through the launch's sink, behind S14's rate cap.
//!
//! Each request passes these checks in order: the token (agent listener only), the head, the
//! form and scheme (405, `Scheme`), the port (403, `Port`), the silent list (403, no denial),
//! the allowlist (403, `NotAllowed`), the route, the address check (direct connections, or an
//! IP literal through an upstream; 403, `PrivateAddress`), then the connect.

mod address;
mod allowlist;
mod connect;
mod head;
mod http;
mod net;
mod relay;
mod server;
mod shared_hosts;
mod target;
mod token;
mod upstream;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use async_io::Async;
use futures::channel::oneshot;
use futures::future::{AbortHandle, AbortRegistration};
use serde::Serialize;

pub use allowlist::{Allowlist, LoginKind, builtin_allowlist};
pub use upstream::{Route, Upstream, upstream_for};

use crate::{Clock, DenialSink, Error, HostRefusal, SystemClock};
use allowlist::Name;
use net::{Network, SystemNetwork};
use token::Token;
use upstream::Lookup;

/// What a launch's proxy allows, where it sends traffic, and where its refusals go.
pub struct ProxyConfig {
    /// The hosts the launch may reach. `allow` and `revoke` change it live.
    pub allow: Allowlist,
    /// Hosts refused with no denial and no card (stage E's S27).
    pub silent: Allowlist,
    /// Straight out, a fixed proxy, or the system's.
    pub upstream: Upstream,
    /// The launch's sink. The proxy puts S14's rate cap in front of it.
    pub denials: Arc<dyn DenialSink>,
    /// Called once per request with what the proxy did (the example's JSON lines). Never
    /// carries the token or a header.
    pub record: Option<Record>,
}

impl fmt::Debug for ProxyConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyConfig")
            .field("allow", &self.allow)
            .field("silent", &self.silent)
            .field("upstream", &self.upstream)
            .field("record", &self.record.is_some())
            .finish_non_exhaustive()
    }
}

/// The per-request callback of [`ProxyConfig::record`]. It runs on the proxy thread.
pub type Record = Arc<dyn Fn(&Exchange) + Send + Sync>;

/// Which listener a request came in on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Listener {
    /// The agent's, which checks the token.
    Agent,
    /// The browser's, which takes requests without one.
    Browser,
}

/// What the proxy did with one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// A CONNECT tunnel was opened.
    Tunnel,
    /// A plain `http://` request was forwarded.
    Forward,
    /// Refused with a denial: `why` says which check.
    Refused,
    /// On the silent list: refused with no denial.
    Silent,
    /// No valid token on the agent listener: 407.
    AuthRequired,
    /// A head the proxy could not read: too large, too slow or malformed.
    BadRequest,
    /// Too many open tunnels: 503.
    Busy,
    /// The upstream proxy asked for credentials: 502.
    UpstreamAuth,
    /// DNS, the connect or the upstream failed: 502 or 504.
    Failed,
}

/// One request, as [`ProxyConfig::record`] gets it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct Exchange {
    /// The listener it came in on.
    pub listener: Listener,
    /// The host asked for (ASCII), or empty when the proxy never read one.
    pub host: String,
    /// The port asked for, or 0.
    pub port: u16,
    /// What the proxy did.
    pub verdict: Verdict,
    /// The check that refused it, for `Refused`.
    pub why: Option<HostRefusal>,
    /// Bytes from the client to the origin (body or tunnel traffic; not the head).
    pub bytes_up: u64,
    /// Bytes from the origin to the client.
    pub bytes_down: u64,
}

/// Timeouts and caps; tests shorten the timeouts.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Limits {
    /// To read the request line and headers.
    pub(crate) head: Duration,
    /// To connect to an origin or an upstream, and for an upstream's answer to a CONNECT.
    pub(crate) connect: Duration,
    /// Open tunnels (and forwarded requests) per launch; the next gets 503.
    pub(crate) tunnels: usize,
    /// Client connections open at once, tunnels included; the next is closed unanswered.
    pub(crate) clients: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            head: Duration::from_secs(10),
            connect: Duration::from_secs(10),
            tunnels: 64,
            clients: 256,
        }
    }
}

/// What tests swap: the clock behind the rate cap, and the limits.
pub(crate) struct Options {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) limits: Limits,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            clock: Arc::new(SystemClock::new()),
            limits: Limits::default(),
        }
    }
}

/// A shared clock as a [`Clock`].
pub(crate) struct SharedClock(Arc<dyn Clock>);

impl Clock for SharedClock {
    fn now(&self) -> Duration {
        self.0.now()
    }
}

/// Starts a launch's proxy on its own thread (`clauseo-proxy`). It runs until the handle drops.
pub fn start_proxy(config: ProxyConfig) -> Result<ProxyHandle, Error> {
    start_with(config, Arc::new(SystemNetwork), Options::default())
}

pub(crate) fn start_with(
    config: ProxyConfig,
    net: Arc<dyn Network>,
    options: Options,
) -> Result<ProxyHandle, Error> {
    let lookup = Arc::new(Lookup::new(config.upstream)?);
    let token = Token::generate()?;
    let bind = || -> Result<(Async<std::net::TcpListener>, u16), Error> {
        let listener = crate::ports::bind_loopback().map_err(Error::ProxyStart)?;
        let port = listener.local_addr().map_err(Error::ProxyStart)?.port();
        Ok((Async::new(listener).map_err(Error::ProxyStart)?, port))
    };
    let (agent, port) = bind()?;
    let (browser, browser_port) = bind()?;
    let shared = Arc::new(Shared::new(config.allow));
    let parts = server::Parts {
        shared: Arc::clone(&shared),
        silent: config.silent,
        token: token.clone(),
        denials: config.denials,
        clock: SharedClock(options.clock),
        net,
        lookup,
        record: config.record,
        limits: options.limits,
    };
    let (stop, stopped) = oneshot::channel();
    let thread = thread::Builder::new()
        .name("clauseo-proxy".to_owned())
        .spawn(move || server::run(parts, &agent, &browser, stopped))
        .map_err(Error::ProxyStart)?;
    Ok(ProxyHandle {
        port,
        browser_port,
        token,
        shared,
        stop: Some(stop),
        thread: Some(thread),
    })
}

/// A running proxy. Dropping it stops the thread, closes both listeners and ends every tunnel.
pub struct ProxyHandle {
    port: u16,
    browser_port: u16,
    token: Token,
    shared: Arc<Shared>,
    stop: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl fmt::Debug for ProxyHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyHandle")
            .field("port", &self.port)
            .field("browser_port", &self.browser_port)
            .field("token", &"<redacted>")
            .finish_non_exhaustive()
    }
}

impl ProxyHandle {
    /// The agent listener's port (in `HTTPS_PROXY`; checks the token).
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The browser listener's port (no token; held to its matter by the jail's port rule).
    pub fn browser_port(&self) -> u16 {
        self.browser_port
    }

    /// The agent's proxy variables: `Some` to set, `None` to remove. Sets `HTTPS_PROXY`,
    /// `HTTP_PROXY`, `https_proxy` and `http_proxy` to `http://clauseo:<token>@127.0.0.1:<port>`,
    /// `NO_PROXY` and `no_proxy` to empty, and removes `ALL_PROXY` and `all_proxy`.
    pub fn env(&self) -> Vec<(OsString, Option<OsString>)> {
        let url = format!(
            "http://{}:{}@127.0.0.1:{}",
            token::USER,
            self.token.hex(),
            self.port
        );
        let set = |name: &str, value: &str| (OsString::from(name), Some(OsString::from(value)));
        vec![
            set("HTTPS_PROXY", &url),
            set("HTTP_PROXY", &url),
            set("https_proxy", &url),
            set("http_proxy", &url),
            set("NO_PROXY", ""),
            set("no_proxy", ""),
            (OsString::from("ALL_PROXY"), None),
            (OsString::from("all_proxy"), None),
        ]
    }

    /// Adds an exact host to the live list (R18: a site the lawyer allowed); the next request
    /// to it passes. A host that is not a valid name is ignored.
    pub fn allow(&self, host: &str) {
        if let Some(name) = Name::parse(host) {
            self.shared.live().allow.insert_exact(name);
        } else {
            tracing::debug!(host, "egress: allow of a non-host ignored");
        }
    }

    /// Removes that exact host from the live list, whether it came from the start list or from
    /// `allow`, and closes every open tunnel whose host the list no longer allows. A `*.domain`
    /// entry that still covers the host keeps it allowed, and its tunnels open.
    pub fn revoke(&self, host: &str) {
        if let Some(name) = Name::parse(host) {
            self.shared.revoke(&name);
        } else {
            tracing::debug!(host, "egress: revoke of a non-host ignored");
        }
    }
}

impl Drop for ProxyHandle {
    fn drop(&mut self) {
        drop(self.stop.take());
        if let Some(thread) = self.thread.take() {
            // A sink that drops the handle runs on the proxy thread, which cannot join itself;
            // it ends once the call returns.
            if thread.thread().id() != thread::current().id() && thread.join().is_err() {
                tracing::warn!("egress: the proxy thread panicked");
            }
        }
    }
}

/// What the handle and the proxy thread share: the live list and the open tunnels, under one
/// lock so a `revoke` cannot miss a tunnel being opened.
pub(crate) struct Shared {
    live: Mutex<Live>,
}

pub(crate) struct Live {
    pub(crate) allow: Allowlist,
    tunnels: BTreeMap<u64, (Name, AbortHandle)>,
    next: u64,
}

/// Why a tunnel was not opened.
pub(crate) enum Unopened {
    /// The list no longer allows the host (a `revoke` came during the connect).
    NotAllowed,
    /// The launch has its most tunnels open.
    Busy,
}

impl Shared {
    fn new(allow: Allowlist) -> Shared {
        Shared {
            live: Mutex::new(Live {
                allow,
                tunnels: BTreeMap::new(),
                next: 0,
            }),
        }
    }

    pub(crate) fn live(&self) -> MutexGuard<'_, Live> {
        // The list and the tunnel table stay valid whatever a panicking thread left.
        self.live.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Opens a tunnel to `name` if the list still allows it and there is room.
    pub(crate) fn open(
        &self,
        name: &Name,
        most: usize,
    ) -> Result<(u64, AbortRegistration), Unopened> {
        let mut live = self.live();
        if !live.allow.contains_name(name) {
            return Err(Unopened::NotAllowed);
        }
        if live.tunnels.len() >= most {
            return Err(Unopened::Busy);
        }
        let (handle, registration) = AbortHandle::new_pair();
        let id = live.next;
        live.next += 1;
        live.tunnels.insert(id, (name.clone(), handle));
        Ok((id, registration))
    }

    pub(crate) fn close(&self, id: u64) {
        self.live().tunnels.remove(&id);
    }

    fn revoke(&self, name: &Name) {
        let mut live = self.live();
        live.allow.remove_exact(name);
        let Live { allow, tunnels, .. } = &mut *live;
        tunnels.retain(|_, (host, handle)| {
            let keep = allow.contains_name(host);
            if !keep {
                handle.abort();
            }
            keep
        });
    }
}

#[cfg(test)]
mod tests;
