//! One file's GET, its redirects followed by hand: at most [`MAX_REDIRECTS`] hops, each
//! `Location` resolved against the URL before it, each URL `https` (when asked) and on an
//! allowed host, the body capped by the client's read limit.
//!
//! Each hop asks its route first, off any lock: `network.upstream_proxy` when set (as agent
//! installs do), else the seams' `Upstream` (the release's: `HTTPS_PROXY` and its kin with
//! `NO_PROXY`, then the OS's proxy settings, PAC included), through
//! `clauseo_sandbox::upstream_for`, as the connectors do. A refused setting fails the fetch and
//! never goes around the proxy. TLS trusts the OS store (the platform verifier), which holds a
//! firm CA installed in the OS; `network.ca_certs` is per agent and is not used here.

use std::cell::RefCell;
use std::time::Duration;

use clauseo_connectors::HttpsClient;
use clauseo_sandbox::{Route, Upstream, upstream_for};
use url::Url;

use super::BlockListUpdate;

/// The most redirects one fetch follows.
const MAX_REDIRECTS: usize = 5;

/// Why a fetch gave no body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Failure {
    /// A 404: nothing is published yet.
    NotPublished,
    /// Anything else, by kind (never the URL).
    Refused(String),
}

/// The `User-Agent` of each fetch.
const USER_AGENT: &str = "Clauseo";

/// The clients of the fetches: one per route, the last one kept for the next request. Used
/// from the `clauseo-blocklist` thread only.
pub(crate) struct Clients {
    https_only: bool,
    last: RefCell<Option<(Option<String>, HttpsClient)>>,
}

impl Clients {
    /// No client yet.
    pub(crate) fn new(https_only: bool) -> Self {
        Self {
            https_only,
            last: RefCell::new(None),
        }
    }

    /// The client for `url` under `upstream`: its route asked now (blocking: the OS may run
    /// PAC; no lock is held), and the last client reused when the route is the same.
    pub(super) fn for_url(&self, upstream: &Upstream, url: &Url) -> Result<HttpsClient, Failure> {
        // The error is not logged: it can quote the proxy setting, password and all.
        let route = upstream_for(upstream, url)
            .map_err(|_unsaid| refused("the proxy setting is refused"))?;
        let proxy = match route {
            Route::Direct => None,
            // An IPv6 literal comes without brackets.
            Route::Http { host, port } if host.contains(':') => {
                Some(format!("http://[{host}]:{port}"))
            }
            Route::Http { host, port } => Some(format!("http://{host}:{port}")),
        };
        let mut last = self.last.borrow_mut();
        if let Some((held, client)) = &*last
            && *held == proxy
        {
            return Ok(client.clone());
        }
        let client = HttpsClient::new(USER_AGENT, self.https_only, proxy.as_deref())
            .map_err(Failure::Refused)?;
        *last = Some((proxy, client.clone()));
        Ok(client)
    }
}

/// The route source of a round: `upstream_proxy` (the setting) when set, else `fallback`.
pub(crate) fn upstream(
    upstream_proxy: Option<&str>,
    fallback: &Upstream,
) -> Result<Upstream, Failure> {
    match upstream_proxy {
        Some(proxy) => Url::parse(proxy)
            .map(Upstream::Fixed)
            .map_err(|_unsaid| refused("the proxy setting is refused")),
        None => Ok(fallback.clone()),
    }
}

/// GETs `start` under `policy` through `upstream`, reading at most `cap` bytes, each hop
/// within `timeout`.
pub(crate) fn fetch(
    (clients, upstream): (&Clients, &Upstream),
    start: &str,
    policy: &BlockListUpdate,
    (cap, timeout): (u64, Duration),
) -> Result<Vec<u8>, Failure> {
    let mut url = Url::parse(start).map_err(|_bad| refused("the URL is not usable"))?;
    for _hop in 0..=MAX_REDIRECTS {
        check(&url, policy)?;
        let got = clients
            .for_url(upstream, &url)?
            .get(url.as_str(), cap, timeout)
            .map_err(Failure::Refused)?;
        match got.status {
            200 => return Ok(got.body),
            404 => return Err(Failure::NotPublished),
            301 | 302 | 303 | 307 | 308 => {
                let location = got
                    .location
                    .ok_or_else(|| refused("a redirect without a Location"))?;
                url = url
                    .join(&location)
                    .map_err(|_bad| refused("a redirect to an unusable URL"))?;
            }
            status => return Err(Failure::Refused(format!("HTTP status {status}"))),
        }
    }
    Err(refused("too many redirects"))
}

/// Whether `url` may be fetched: its scheme, no explicit port when `https_only` (443 parses as
/// none), no user info, and its host in the allowed set.
pub(super) fn check(url: &Url, policy: &BlockListUpdate) -> Result<(), Failure> {
    let scheme = url.scheme();
    if scheme != "https" && (policy.https_only || scheme != "http") {
        return Err(refused("a URL that is not https"));
    }
    if policy.https_only && url.port().is_some() {
        return Err(refused("a URL with a port"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(refused("a URL with user info"));
    }
    let host = url.host_str().unwrap_or_default();
    if !policy
        .allowed_hosts
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(host))
    {
        return Err(refused("a host not allowed"));
    }
    Ok(())
}

/// A [`Failure::Refused`] saying `why`.
fn refused(why: &str) -> Failure {
    Failure::Refused(why.to_owned())
}
