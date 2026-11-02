//! Where the proxy sends a connection: straight to the origin, or through the firm's proxy.
//!
//! Stub: `Direct` and `Fixed` work; `System` behaves as `Direct` until the upstream writer's
//! discovery (the host's `HTTPS_PROXY`, then the OS's proxy settings or PAC) replaces this file.

use crate::Error;

/// Which upstream the proxy uses.
#[derive(Debug, Clone)]
pub enum Upstream {
    /// Connect to origins directly.
    Direct,
    /// Always this proxy: `http://host[:port]` (`network.upstream_proxy`).
    Fixed(url::Url),
    /// The host's `HTTPS_PROXY`, then the OS's settings or PAC.
    System,
}

/// The way to one origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    /// Connect to the origin.
    Direct,
    /// Through an HTTP proxy at `host:port` (`host` is a name or an address, an IPv6 one
    /// possibly in brackets).
    Http {
        /// The proxy's host.
        host: String,
        /// The proxy's port.
        port: u16,
    },
}

/// The upstream, checked once, answering per origin.
#[derive(Debug)]
pub(crate) struct Lookup {
    fixed: Option<(String, u16)>,
}

impl Lookup {
    /// Refuses a Fixed URL that is not `http://host[:port]` with no userinfo, path, query or
    /// fragment.
    pub(crate) fn new(upstream: Upstream) -> Result<Lookup, Error> {
        let fixed = match upstream {
            Upstream::Direct | Upstream::System => None,
            Upstream::Fixed(url) => Some(fixed_proxy(&url)?),
        };
        Ok(Lookup { fixed })
    }

    /// Blocking (the OS may run PAC): call it off the I/O thread. `host` is the origin's name
    /// in ASCII, or its address (an IPv6 one without brackets).
    pub(crate) fn route(&self, _host: &str, _port: u16) -> Route {
        match &self.fixed {
            Some((host, port)) => Route::Http {
                host: host.clone(),
                port: *port,
            },
            None => Route::Direct,
        }
    }
}

/// The route for the host's own HTTPS to `url` (stage E's connector relay).
pub fn upstream_for(upstream: &Upstream, url: &url::Url) -> Route {
    match upstream {
        Upstream::Direct | Upstream::System => Route::Direct,
        Upstream::Fixed(proxy) => match fixed_proxy(proxy) {
            Ok((host, port)) => Route::Http { host, port },
            Err(error) => {
                tracing::warn!(%error, origin = url.host_str(), "egress: upstream refused; direct");
                Route::Direct
            }
        },
    }
}

fn fixed_proxy(url: &url::Url) -> Result<(String, u16), Error> {
    let refuse = |why: &str| Error::ProxyConfig(format!("upstream proxy refused: {why}"));
    if url.scheme() != "http" {
        return Err(refuse("not an http:// URL"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(refuse("it carries a user name or password"));
    }
    if !matches!(url.path(), "" | "/") || url.query().is_some() || url.fragment().is_some() {
        return Err(refuse("it has a path, query or fragment"));
    }
    let host = url.host_str().ok_or_else(|| refuse("no host"))?;
    let port = url
        .port_or_known_default()
        .ok_or_else(|| refuse("no port"))?;
    Ok((host.to_owned(), port))
}
