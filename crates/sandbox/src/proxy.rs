//! The egress proxy: one per agent launch, the jailed agent's only way out and the browser's way
//! out in every tier. It listens on loopback for the agent, and for the browser too when the
//! launch has one ([`ProxyConfig::browser`]; never on tier B, decision D10).
//!
//! It allows a CONNECT to port 443 of a host on the launch's allowlist and a plain `http://`
//! request to port 80 of one, chains to the firm's proxy when there is one, never looks inside
//! TLS (it holds no CA), refuses loopback and private addresses, and turns each refusal into a
//! [`Denial`](crate::Denial) through the launch's sink, behind S14's rate cap.
//!
//! Each request passes these checks in order: the head, the token (agent listener only), the
//! form and scheme (405, `Scheme`; 400 when there is no host to name), the silent list (403, no
//! denial), the block list (403, `Blocked`, by name or IP literal, no DNS; it beats every
//! allowlist entry; a host the lawyer allowed, exactly, skips it), the port (403, `Port`), an
//! IP literal that is not public or a `localhost` name
//! (403, `PrivateAddress`; no allowlist entry lets one through), the allowlist (403,
//! `NotAllowed`), the route, the address check (403, `PrivateAddress`: every resolved address
//! on a direct connection; through an upstream, an IP literal, or a name's local answers when
//! local DNS gives any), then the connect.
//!
//! A sink or record callback runs on the proxy thread and must not block: one that blocks stalls
//! every connection of the launch, and `ProxyHandle`'s drop, which joins the thread.

mod address;
mod allowlist;
mod connect;
mod head;
mod http;
mod load;
mod net;
mod relay;
mod server;
mod shared_hosts;
mod target;
mod token;
pub mod upstream;

use crate::ports::ListenerFor;
use std::collections::{BTreeMap, BTreeSet};
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
pub use upstream::{
    FailKind, Route, UnusableProxy, Upstream, UpstreamAnswer, launch_route, upstream_answer,
    upstream_for,
};

use crate::{Clock, DenialSink, Error, HostRefusal, SystemClock};
use allowlist::Name;
use net::{Network, SystemNetwork};
use token::Token;
use upstream::{Env, Intent, Lookup, OsAnswer, Reach};

/// What a launch's proxy allows, where it sends traffic, and where its refusals go.
pub struct ProxyConfig {
    /// The hosts the launch may reach. `allow` and `revoke` change it live.
    pub allow: Allowlist,
    /// Hosts refused with no denial and no card (stage E's S27).
    pub silent: Allowlist,
    /// The unsafe-site block list (ADR 0053), shared with the host's updater: each request
    /// reads the list in use then, so an update reaches the next request and closes nothing.
    /// Hosts on it are refused with `Denial::Blocked` whatever the allowlist says. Unloaded,
    /// nothing is blocked.
    pub block: Arc<clauseo_blocklist::Current>,
    /// The hosts the lawyer allowed despite the block list (ADR 0053, PR 2): the host's live
    /// view for this launch's matter, its `Matter` allows and the `Always` ones, empty while
    /// policy forbids overrides. Asked before the list, exactly by host; an allowed host is
    /// then checked as any other (silent list, port, private address, allowlist). A change
    /// reaches the next request, and a revoke closes the open tunnels and forwarded requests to
    /// a host it removed that the list still names. Empty by default.
    pub allowed: Arc<clauseo_blocklist::Allowed>,
    /// Straight out, a fixed proxy, or the system's.
    pub upstream: Upstream,
    /// The launch's sink. The proxy puts S14's rate cap in front of it.
    pub denials: Arc<dyn DenialSink>,
    /// Called once per request with what the proxy did (the example's JSON lines). Never
    /// carries the token or a header.
    pub record: Option<Record>,
    /// Whether to bind the browser listener; false from [`ProxyConfig::new`]. The host sets it
    /// from whether this session has the browser (the browser entry it loads as available),
    /// never from the policy's [`Loopback`](crate::Loopback), which also carries non-browser
    /// ports such as the case-law relay's. That listener takes no token, so a launch without
    /// the browser (tier B, decision D10) must not open one: another matter's agent could send
    /// traffic through it under this launch's list.
    pub browser: bool,
    /// Who the agent listener is for: [`ListenerFor::TierB`] for a tier-B launch, whose agent
    /// reaches only `ports::TIER_B_PORTS` on Windows; [`ListenerFor::Other`] (from
    /// [`ProxyConfig::new`]) for any other. The browser listener, which takes no token, is
    /// always bound as `Other`, outside the range every pool user reaches (S15 PR7).
    pub listener: ListenerFor,
}

impl ProxyConfig {
    /// A config with no record callback, no browser listener, an unloaded block list, no
    /// allowed site, and its agent listener outside tier B's range ([`ListenerFor::Other`]).
    pub fn new(
        allow: Allowlist,
        silent: Allowlist,
        upstream: Upstream,
        denials: Arc<dyn DenialSink>,
    ) -> ProxyConfig {
        ProxyConfig {
            allow,
            silent,
            block: Arc::default(),
            allowed: Arc::default(),
            upstream,
            denials,
            record: None,
            browser: false,
            listener: ListenerFor::Other,
        }
    }
}

impl fmt::Debug for ProxyConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProxyConfig")
            .field("allow", &self.allow)
            .field("silent", &self.silent)
            .field("upstream", &self.upstream)
            .field("record", &self.record.is_some())
            .field("browser", &self.browser)
            .field("listener", &self.listener)
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
    /// On the block list: refused with `Denial::Blocked`.
    Blocked,
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
    /// With no byte moved either way, a tunnel or a forwarded request is closed. Agents keep
    /// long streaming responses open, so it is long.
    pub(crate) idle: Duration,
    /// Open tunnels (and forwarded requests) per launch; the next gets 503.
    pub(crate) tunnels: usize,
    /// Client connections still reading their head, per listener; the next is closed unanswered.
    pub(crate) pending: usize,
    /// Client and origin sockets open at once, per launch. A client over it is closed
    /// unanswered; an origin connection over it gets the client 503.
    pub(crate) sockets: usize,
    /// Client and origin sockets open at once across every proxy in the process, the same way.
    /// `start_proxy` lowers it to half the process's soft open-file limit when that is smaller.
    pub(crate) process: usize,
    /// After an error answer, how long the proxy reads and discards what the client still sends
    /// before it closes, so the close does not reset the connection and lose the answer.
    pub(crate) linger: Duration,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            head: Duration::from_secs(10),
            connect: Duration::from_secs(10),
            idle: Duration::from_mins(15),
            tunnels: 64,
            pending: 64,
            // The pending clients, plus a client and an origin socket per tunnel.
            sockets: 64 + 2 * 64,
            process: 1024,
            linger: Duration::from_secs(1),
        }
    }
}

/// What tests swap: the clock behind the rate cap, the limits, and for `Upstream::System` the
/// host's proxy variables, the OS lookup and what the settings show when a lookup fails without
/// one (setting the process's environment in a test is `unsafe` in edition 2024, and the crate
/// denies unsafe code). A swapped OS lookup runs on `clock` too. `reach` probes a proxy in place
/// of [`upstream::reachable`].
pub(crate) struct Options {
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) limits: Limits,
    pub(crate) system: Option<(Env, Box<OsAnswer>, Box<Intent>)>,
    pub(crate) reach: Option<Box<Reach>>,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            clock: Arc::new(SystemClock::new()),
            limits: Limits::default(),
            system: None,
            reach: None,
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
    let mut options = Options::default();
    options.limits.process = load::process_cap(load::soft_open_files());
    start_with(config, Arc::new(SystemNetwork), options)
}

pub(crate) fn start_with(
    config: ProxyConfig,
    net: Arc<dyn Network>,
    options: Options,
) -> Result<ProxyHandle, Error> {
    let lookup = match options.system {
        Some((env, os, intent)) => {
            let clock = SharedClock(Arc::clone(&options.clock));
            Lookup::with_parts(config.upstream, env, os, clock)?.with_intent(intent)
        }
        None => Lookup::new(config.upstream)?,
    };
    let lookup = Arc::new(match options.reach {
        Some(reach) => lookup.with_reach(reach),
        None => lookup,
    });
    let token = Token::generate()?;
    let bind = |whom: ListenerFor| -> Result<(Async<std::net::TcpListener>, u16), Error> {
        let listener = crate::ports::bind_loopback(whom).map_err(Error::ProxyStart)?;
        let port = listener.local_addr().map_err(Error::ProxyStart)?.port();
        Ok((Async::new(listener).map_err(Error::ProxyStart)?, port))
    };
    let (agent, port) = bind(config.listener)?;
    let (browser, browser_port) = if config.browser {
        // No token on this one, so never where a pool user's static permit reaches.
        let (listener, port) = bind(ListenerFor::Other)?;
        (Some(listener), Some(port))
    } else {
        (None, None)
    };
    if config.block.first_unloaded_notice() {
        tracing::warn!("egress: the block list is not loaded; no site is blocked");
    }
    let shared = Arc::new(Shared::new(config.allow));
    let watcher = revoke_watcher(&shared, &config.block, &config.allowed);
    config.allowed.watch(&watcher);
    let parts = server::Parts {
        shared: Arc::clone(&shared),
        silent: config.silent,
        block: config.block,
        allowed: config.allowed,
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
        .spawn(move || server::run(parts, &agent, browser.as_ref(), stopped))
        .map_err(Error::ProxyStart)?;
    Ok(ProxyHandle {
        port,
        browser_port,
        token,
        shared,
        _watcher: watcher,
        stop: Some(stop),
        thread: Some(thread),
    })
}

/// The watcher on the launch's allowed set: when a revoke removes hosts, it closes every open
/// tunnel and forwarded request whose host is among them, is not allowed again, and is still on
/// the block list. It runs on the revoking thread, after the swap, and does no I/O.
fn revoke_watcher(
    shared: &Arc<Shared>,
    block: &Arc<clauseo_blocklist::Current>,
    allowed: &Arc<clauseo_blocklist::Allowed>,
) -> Arc<clauseo_blocklist::Watcher> {
    let shared = Arc::downgrade(shared);
    let block = Arc::clone(block);
    let allowed = Arc::downgrade(allowed);
    Arc::new(move |removed: &BTreeSet<String>| {
        let (Some(shared), Some(allowed)) = (shared.upgrade(), allowed.upgrade()) else {
            return;
        };
        let list = block.get();
        let now = allowed.get();
        shared.close_where(|name| {
            let text = name.text();
            removed.contains(&text) && !now.contains(&text) && server::listed(&list, name).is_some()
        });
    })
}

/// A running proxy. Dropping it stops the thread, closes its listeners and ends every tunnel.
pub struct ProxyHandle {
    port: u16,
    browser_port: Option<u16>,
    token: Token,
    shared: Arc<Shared>,
    /// Closes open tunnels to a host whose allow (`ProxyConfig::allowed`) is revoked; held so
    /// it lives as long as the proxy.
    _watcher: Arc<clauseo_blocklist::Watcher>,
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

    /// The browser listener's port (no token; held to its matter by the jail's port rule), or
    /// `None` when [`ProxyConfig::browser`] was false and no browser listener was bound.
    pub fn browser_port(&self) -> Option<u16> {
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
    /// The lawyer's allow for the host was revoked since the block list was asked, and the
    /// list still names it.
    Blocked,
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

    /// Opens a tunnel to `name` if the list still allows it, `still` (asked under the lock,
    /// so a revoke's watcher cannot miss the tunnel; it must do no I/O) says the block list
    /// still lets it through, and there is room.
    pub(crate) fn open(
        &self,
        name: &Name,
        most: usize,
        still: impl FnOnce(&Name) -> bool,
    ) -> Result<(u64, AbortRegistration), Unopened> {
        let mut live = self.live();
        if !live.allow.contains_name(name) {
            return Err(Unopened::NotAllowed);
        }
        if !still(name) {
            return Err(Unopened::Blocked);
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

    /// Closes every open tunnel whose host `close` names.
    fn close_where(&self, close: impl Fn(&Name) -> bool) {
        self.live().tunnels.retain(|_, (host, handle)| {
            let shut = close(host);
            if shut {
                handle.abort();
            }
            !shut
        });
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
