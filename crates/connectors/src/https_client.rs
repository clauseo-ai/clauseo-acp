//! The crate's HTTP client setup lent to another crate: plain GETs of public files, such as the
//! host's block list (ADR 0053). Same TLS and timeouts as the connectors' agent (`http.rs`);
//! redirects are returned, never followed, so the caller checks each hop.

use std::time::Duration;

use ureq::{Agent, Proxy};

use crate::http::{CONNECT_TIMEOUT, REQUEST_TIMEOUT, describe, tls};

/// A ureq agent over rustls with the `ring` provider and the OS trust store. Cheap to clone.
#[derive(Clone)]
pub struct HttpsClient {
    agent: Agent,
}

/// One answer: its status, its `Location` (on a redirect) and its body (on a 200 only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Got {
    /// The HTTP status.
    pub status: u16,
    /// The `Location` header, as sent.
    pub location: Option<String>,
    /// The body of a 200; empty for any other status (it is not read).
    pub body: Vec<u8>,
}

impl HttpsClient {
    /// A client sending `user_agent`, through `proxy` (`http://host:port`; `None` is direct,
    /// the environment is not read: the caller asks the route). With `https_only`, a plain
    /// `http` URL is refused. Fails when ureq cannot use `proxy`; the error does not repeat it.
    pub fn new(
        user_agent: &str,
        https_only: bool,
        proxy: Option<&str>,
    ) -> Result<HttpsClient, String> {
        let proxy = match proxy {
            Some(proxy) => Some(
                Proxy::new(proxy)
                    .map_err(|_unsaid| "the proxy address is not usable".to_owned())?,
            ),
            None => None,
        };
        let agent = Agent::config_builder()
            .tls_config(tls())
            .proxy(proxy)
            .https_only(https_only)
            .max_redirects(0)
            .http_status_as_error(false)
            .user_agent(user_agent)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .timeout_global(Some(REQUEST_TIMEOUT))
            .build()
            .new_agent();
        Ok(HttpsClient { agent })
    }

    /// GETs `url`, reading at most `limit` bytes of a 200's body (more is an error), the
    /// whole request within `timeout` (the connectors' agent allows 60 s). The error names the
    /// kind of failure only, never the URL.
    pub fn get(&self, url: &str, limit: u64, timeout: Duration) -> Result<Got, String> {
        let mut response = self
            .agent
            .get(url)
            .config()
            .timeout_global(Some(timeout))
            .build()
            .call()
            .map_err(|err| describe(&err))?;
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get("location")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = if status == 200 {
            response
                .body_mut()
                .with_config()
                .limit(limit)
                .read_to_vec()
                .map_err(|err| describe(&err))?
        } else {
            Vec::new()
        };
        Ok(Got {
            status,
            location,
            body,
        })
    }
}
