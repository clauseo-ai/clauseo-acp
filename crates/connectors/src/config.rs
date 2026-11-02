//! [`Config`]: where the connector is, how to reach it, the timeouts and the clock.

use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use crate::error::Error;
use crate::known::{KnownConnector, PRODUCTION_ORIGIN, STAGING_ORIGIN};

#[cfg(feature = "test-support")]
mod manual;

#[cfg(feature = "test-support")]
pub use manual::ManualClock;

/// The sign-in timeout: the lawyer's time in the browser.
pub(crate) const SIGNIN_TIMEOUT: Duration = Duration::from_mins(5);
/// How long `wait_recharge` polls before `RechargeTimedOut`.
pub(crate) const RECHARGE_DEADLINE: Duration = Duration::from_mins(15);

/// How the crate reaches the connector, and where it keeps its state.
#[derive(Clone)]
pub struct Config {
    /// Which connector host: production, staging, or (tests) the fake.
    pub origin: Origin,
    /// `Clauseo/<app version>`, sent on every request.
    pub user_agent: String,
    /// The proxy for every request.
    pub proxy: ProxyChoice,
    /// How long `finish` waits for the browser's callback.
    pub signin_timeout: Duration,
    /// How long `wait_recharge` polls.
    pub recharge_deadline: Duration,
    /// The one source of time.
    pub clock: Arc<dyn Clock>,
    /// `<data_dir>/connectors/state.json`.
    pub state_file: PathBuf,
}

impl Config {
    /// Production, the system clock, the proxy from the environment and the default timeouts.
    pub fn production(user_agent: String, state_file: PathBuf) -> Config {
        Config {
            origin: Origin::Production,
            user_agent,
            proxy: ProxyChoice::Env,
            signin_timeout: SIGNIN_TIMEOUT,
            recharge_deadline: RECHARGE_DEADLINE,
            clock: Arc::new(SystemClock),
            state_file,
        }
    }
}

impl fmt::Debug for Config {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Config")
            .field("origin", &self.origin)
            .field("user_agent", &self.user_agent)
            .field("proxy", &self.proxy)
            .field("signin_timeout", &self.signin_timeout)
            .field("recharge_deadline", &self.recharge_deadline)
            .field("state_file", &self.state_file)
            .finish_non_exhaustive()
    }
}

/// The connector's host. Every compiled-in URL starts with `https://data.clauseo.chat`; under
/// `Staging` and `Fake` that prefix is replaced by the origin's, keeping the path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// `https://data.clauseo.chat`.
    Production,
    /// `https://data-staging.clauseo.chat`, the same paths as production.
    Staging,
    /// The fake connector at `http://127.0.0.1:<port>` (feature `test-support`); any other URL
    /// is refused before any request (`Error::BadOrigin`).
    #[cfg(feature = "test-support")]
    Fake(url::Url),
}

impl Origin {
    /// Refuses an origin the crate must never talk to: a `Fake` that is not
    /// `http://127.0.0.1:<port>` with no path, query, fragment or user info.
    pub(crate) fn check(&self) -> Result<(), Error> {
        match self {
            Origin::Production | Origin::Staging => Ok(()),
            #[cfg(feature = "test-support")]
            Origin::Fake(url) => {
                let loopback = url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && matches!(url.path(), "" | "/")
                    && url.query().is_none()
                    && url.fragment().is_none();
                if loopback {
                    Ok(())
                } else {
                    Err(Error::BadOrigin)
                }
            }
        }
    }

    /// The origin's scheme, host and port, without a trailing `/`: `https://data.clauseo.chat`,
    /// `https://data-staging.clauseo.chat` or `http://127.0.0.1:<port>`.
    pub(crate) fn prefix(&self) -> String {
        match self {
            Origin::Production => PRODUCTION_ORIGIN.to_owned(),
            Origin::Staging => STAGING_ORIGIN.to_owned(),
            #[cfg(feature = "test-support")]
            Origin::Fake(url) => url.origin().ascii_serialization(),
        }
    }

    /// A compiled-in URL mapped onto this origin: `https://data.clauseo.chat` at its start is
    /// replaced by [`Origin::prefix`], the path kept. Any other string comes back unchanged.
    pub(crate) fn map_url(&self, compiled_in: &str) -> String {
        match compiled_in.strip_prefix(PRODUCTION_ORIGIN) {
            Some(rest) if rest.is_empty() || rest.starts_with('/') => {
                format!("{}{rest}", self.prefix())
            }
            _ => compiled_in.to_owned(),
        }
    }

    /// Every URL of `connector`, mapped onto this origin.
    pub(crate) fn urls(&self, connector: &KnownConnector) -> ConnectorUrls {
        ConnectorUrls {
            issuer: self.map_url(connector.issuer),
            mcp_url: self.map_url(connector.mcp_url),
            api_base: self.map_url(connector.api_base),
            resources: connector
                .resources
                .iter()
                .map(|resource| self.map_url(resource))
                .collect(),
            client_id: self.map_url(crate::known::CLIENT_ID),
            pricing_url: self.map_url(crate::known::PRICING_URL),
            pay_prefix: self.map_url(connector.pay_prefix),
            protected_resource_metadata: format!(
                "{}/.well-known/oauth-protected-resource{}",
                self.prefix(),
                path_of(connector.mcp_url)
            ),
            authorization_server_metadata: format!(
                "{}/.well-known/oauth-authorization-server",
                self.prefix()
            ),
        }
    }
}

/// The path of a compiled-in URL: what follows the production origin.
fn path_of(compiled_in: &str) -> &str {
    compiled_in
        .strip_prefix(PRODUCTION_ORIGIN)
        .unwrap_or(compiled_in)
}

/// A connector's URLs under one origin ([`Origin::urls`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConnectorUrls {
    /// The issuer, compared exactly with the metadata's `issuer` and the callback's `iss`.
    pub(crate) issuer: String,
    /// The MCP endpoint.
    pub(crate) mcp_url: String,
    /// The JSON API's base.
    pub(crate) api_base: String,
    /// The OAuth `resource` values: MCP, then API.
    pub(crate) resources: Vec<String>,
    /// The OAuth client ID.
    pub(crate) client_id: String,
    /// `pricing.json`.
    pub(crate) pricing_url: String,
    /// The start of every pay link on this origin.
    pub(crate) pay_prefix: String,
    /// `<origin>/.well-known/oauth-protected-resource/mcp/<id>` (RFC 9728).
    pub(crate) protected_resource_metadata: String,
    /// `<origin>/.well-known/oauth-authorization-server` (RFC 8414).
    pub(crate) authorization_server_metadata: String,
}

/// The proxy every request goes through.
#[derive(Clone, PartialEq, Eq)]
pub enum ProxyChoice {
    /// `HTTPS_PROXY` from the environment, until stage C's upstream lookup exists.
    Env,
    /// This proxy.
    Fixed(url::Url),
    /// No proxy.
    Direct,
}

/// `Fixed(<scheme>://<host>:<port>)`: never the user info (a proxy password), path or query.
impl fmt::Debug for ProxyChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ProxyChoice::Env => f.write_str("Env"),
            ProxyChoice::Direct => f.write_str("Direct"),
            ProxyChoice::Fixed(url) => {
                let host = url.host_str().unwrap_or("?");
                match url.port_or_known_default() {
                    Some(port) => write!(f, "Fixed({}://{host}:{port})", url.scheme()),
                    None => write!(f, "Fixed({}://{host})", url.scheme()),
                }
            }
        }
    }
}

/// The crate's one source of wall time.
pub trait Clock: Send + Sync + 'static {
    /// The current time.
    fn now(&self) -> SystemTime;
}

/// The OS clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    #[expect(clippy::disallowed_methods, reason = "the crate's one clock read")]
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[cfg(test)]
mod tests;
