//! The crate's one HTTP client: a ureq agent over rustls with the `ring` provider and the OS trust
//! store (rustls-platform-verifier), so a firm's own root certificate works. Connect timeout 10 s,
//! whole request 60 s (the connector caps a call at 45 s); a `wait=40` poll takes 50 s, set on
//! its own request. Redirects are not followed, a status is not an error (the
//! caller reads it), and production and staging allow `https` only.

use std::sync::Arc;
use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, Proxy, RequestExt as _};
use zeroize::Zeroizing;

use crate::config::{Config, Origin, ProxyChoice};
use crate::error::Error;

/// How long a connection may take.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a whole request may take.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_mins(1);
/// How long a `wait=40` recharge poll may take.
pub(crate) const POLL_TIMEOUT: Duration = Duration::from_secs(50);

/// The agent for `config`. Fails only on a `ProxyChoice::Fixed` that ureq cannot use; the error
/// does not repeat the proxy URL, which may hold credentials.
pub(crate) fn agent(config: &Config) -> Result<Agent, Error> {
    let tls = TlsConfig::builder()
        .provider(TlsProvider::Rustls)
        .root_certs(RootCerts::PlatformVerifier)
        .unversioned_rustls_crypto_provider(Arc::new(rustls::crypto::ring::default_provider()))
        .build();
    let proxy = match &config.proxy {
        ProxyChoice::Env => Proxy::try_from_env(),
        ProxyChoice::Fixed(url) => match Proxy::new(url.as_str()) {
            Ok(proxy) => Some(proxy),
            Err(_) => return Err(Error::Network("the proxy URL is not usable".to_owned())),
        },
        ProxyChoice::Direct => None,
    };
    let https_only = match config.origin {
        Origin::Production | Origin::Staging => true,
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
