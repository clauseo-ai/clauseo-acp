//! The crate's one HTTP client: a ureq agent over rustls with the `ring` provider and the OS trust
//! store (rustls-platform-verifier), so a firm's own root certificate works. Connect timeout 10 s,
//! whole request 60 s (the connector caps a call at 45 s); a `wait=40` poll takes 50 s, set on
//! its own request. Redirects are not followed, a status is not an error (the
//! caller reads it), and production and staging allow `https` only.

use std::sync::Arc;
use std::time::Duration;

use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, Proxy};

use crate::config::{Config, Origin, ProxyChoice};
use crate::error::Error;

/// How long a connection may take.
pub(crate) const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
/// How long a whole request may take.
pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_mins(1);

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

#[cfg(test)]
#[cfg(feature = "test-support")]
mod tests;
