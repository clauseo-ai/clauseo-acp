//! The crate's one HTTP client: a ureq agent over rustls with the `ring` provider and the OS trust
//! store (rustls-platform-verifier), so a firm's own root certificate works. Connect timeout 10 s,
//! whole request 60 s (the connector caps a call at 45 s); a `wait=40` poll takes 50 s, set on
//! its own request. Redirects are not followed, a status is not an error (the
//! caller reads it), and production and staging allow `https` only.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, Proxy, RequestExt as _};
use zeroize::Zeroizing;

use crate::config::{Config, Origin, ProxyChoice, ProxyRefused, ProxyResolver};
use crate::error::Error;

/// How long a connection may take.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a whole request may take.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_mins(1);

/// The agent for one request, or why there is none (a refused proxy setting).
pub(crate) type Route = Result<Agent, String>;

/// A route the resolver gave, and its agent.
type Routed = (ProxyChoice, Agent);

/// The HTTP agents of one [`Config`]: the one agent, or (with `ProxyChoice::Resolve`) the
/// agent of the route the resolver gave last, made again when the route changes.
pub(crate) struct Agents {
    config: Config,
    resolved: Option<(ProxyResolver, Mutex<Option<Routed>>)>,
    fixed: Option<Agent>,
}

impl Agents {
    /// The agents for `config`; asks no resolver. Fails as [`agent`] does.
    pub(crate) fn new(config: &Config) -> Result<Agents, Error> {
        Ok(match &config.proxy {
            ProxyChoice::Resolve(resolver) => Agents {
                config: config.clone(),
                resolved: Some((resolver.clone(), Mutex::new(None))),
                fixed: None,
            },
            _ => Agents {
                config: config.clone(),
                resolved: None,
                fixed: Some(agent(config)?),
            },
        })
    }

    /// The agent for a request now: the route asked of the resolver, if any (outside the lock,
    /// since it can block). A refused route is an error, never a direct request. A caller that
    /// takes a lock around a request (the credential's, the price list's, discovery's) asks
    /// this first and passes the [`Route`] in, so no lookup ever runs under one of those locks.
    pub(crate) fn route(&self) -> Route {
        let Some((resolver, last)) = &self.resolved else {
            return self.fixed.clone().ok_or_else(|| "no HTTP agent".to_owned());
        };
        let choice = resolver
            .resolve()
            .map_err(|ProxyRefused| "the proxy setting is refused".to_owned())?;
        let mut last = crate::connectors::lock(last);
        if let Some((held, agent)) = &*last
            && *held == choice
        {
            return Ok(agent.clone());
        }
        let made = agent_via(&self.config, &choice).map_err(|error| error.to_string())?;
        // `ProxyChoice`'s `Debug` prints no user info.
        tracing::info!(proxy = ?choice, "the connectors' route");
        *last = Some((choice, made.clone()));
        Ok(made)
    }
}

/// The agent for `config`. Fails only on a `ProxyChoice::Fixed` that ureq cannot use (or a
/// `Resolve`, which takes [`Agents`]); the error does not repeat the proxy URL, which may hold
/// credentials.
pub(crate) fn agent(config: &Config) -> Result<Agent, Error> {
    agent_via(config, &config.proxy)
}

/// The agent for `config` through `proxy`.
fn agent_via(config: &Config, proxy: &ProxyChoice) -> Result<Agent, Error> {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let proxy = match proxy {
        ProxyChoice::Env => Proxy::try_from_env(),
        ProxyChoice::Fixed(url) => match Proxy::new(url.as_str()) {
            Ok(proxy) => Some(proxy),
            Err(_) => return Err(Error::Network("the proxy URL is not usable".to_owned())),
        },
        ProxyChoice::Direct => None,
        ProxyChoice::Resolve(_) => {
            return Err(Error::Network(
                "the proxy is resolved per request".to_owned(),
            ));
        }
    };
    let https_only = match config.origin {
        Origin::Production => true,
        #[cfg(feature = "staging")]
        Origin::Staging => true,
        // The local test server is plain http, and its origin is exactly that one.
        #[cfg(feature = "local")]
        Origin::Local => false,
        #[cfg(feature = "test-support")]
        Origin::Fake(_) => false,
    };
    Ok(Agent::config_builder()
        .tls_config(tls)
        .proxy(proxy)
        .https_only(https_only)
        .max_redirects(0)
        .http_status_as_error(false)
        .user_agent(config.user_agent.as_str())
        .timeout_connect(Some(CONNECT_TIMEOUT))
        .timeout_global(Some(REQUEST_TIMEOUT))
        .build()
        .new_agent())
}

/// The most of an answer's body the crate reads.
const MAX_BODY: u64 = 8 * 1024 * 1024;

/// A request body and its content type.
#[derive(Clone, Copy)]
pub(crate) enum SendBody<'a> {
    /// No body.
    None,
    /// `application/x-www-form-urlencoded`.
    Form(&'a [u8]),
    /// `application/json`.
    Json(&'a [u8]),
}

/// One answer, read whole. The body is zeroed on drop: a token answer holds tokens.
pub(crate) struct Reply {
    /// The HTTP status.
    pub(crate) status: u16,
    /// `Content-Type`, lowercased, without parameters.
    pub(crate) content_type: Option<String>,
    /// `Mcp-Session-Id`.
    pub(crate) session_id: Option<String>,
    /// The body.
    pub(crate) body: Zeroizing<Vec<u8>>,
}

/// [`send`] with the agent [`Agents`] gives for this request (no lock may be held).
pub(crate) fn send_via(
    agents: &Agents,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: SendBody<'_>,
    timeout: Duration,
) -> Result<Reply, String> {
    send_on(&agents.route(), method, url, headers, body, timeout)
}

/// [`send`] on a route asked for before (see [`Agents::route`]).
pub(crate) fn send_on(
    route: &Route,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: SendBody<'_>,
    timeout: Duration,
) -> Result<Reply, String> {
    let agent = route.as_ref().map_err(Clone::clone)?;
    send(agent, method, url, headers, body, timeout)
}

/// Sends one request and reads the whole answer. `headers` may hold an `Authorization` value;
/// it is never logged by this crate, and ureq redacts it in its own records. The error says
/// what kind of failure it was, never the URL or a header.
pub(crate) fn send(
    agent: &Agent,
    method: &str,
    url: &str,
    headers: &[(&str, &str)],
    body: SendBody<'_>,
    timeout: Duration,
) -> Result<Reply, String> {
    let mut builder = ureq::http::Request::builder().method(method).uri(url);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let bytes: &[u8] = match body {
        SendBody::None => &[],
        SendBody::Form(bytes) => {
            builder = builder.header("Content-Type", "application/x-www-form-urlencoded");
            bytes
        }
        SendBody::Json(bytes) => {
            builder = builder.header("Content-Type", "application/json");
            bytes
        }
    };
    let request = builder
        .body(bytes)
        .map_err(|_bad_request| "the request could not be built".to_owned())?;
    let mut response = request
        .with_agent(agent)
        .configure()
        .timeout_global(Some(timeout))
        .run()
        .map_err(|err| describe(&err))?;
    let header = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned)
    };
    let content_type = header("content-type").map(|value| {
        value
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase()
    });
    let session_id = header("mcp-session-id");
    let status = response.status().as_u16();
    let body = response
        .body_mut()
        .with_config()
        .limit(MAX_BODY)
        .read_to_vec()
        .map_err(|err| describe(&err))?;
    Ok(Reply {
        status,
        content_type,
        session_id,
        body: Zeroizing::new(body),
    })
}

/// A ureq error by kind only: some of ureq's messages repeat the URL.
fn describe(err: &ureq::Error) -> String {
    match err {
        ureq::Error::Timeout(_) => "timed out",
        ureq::Error::HostNotFound => "host not found",
        ureq::Error::ConnectionFailed => "connection failed",
        ureq::Error::Io(io) => match io.kind() {
            std::io::ErrorKind::ConnectionRefused => "connection refused",
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => "timed out",
            _ => "connection error",
        },
        ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "TLS failed",
        ureq::Error::RequireHttpsOnly(_) => "https required",
        ureq::Error::BodyExceedsLimit(_) => "answer too large",
        ureq::Error::ConnectProxyFailed(_) | ureq::Error::InvalidProxyUrl => "proxy failed",
        _ => "request failed",
    }
    .to_owned()
}

#[cfg(test)]
#[cfg(feature = "test-support")]
mod tests;
