//! The route a program takes by the proxy variables it was launched with: the one the host's
//! offline probe stands on (D-050), read from the agent's final launch environment, with the
//! same reading of a proxy URL and of `NO_PROXY` that [`Upstream::System`] gives the host's own
//! variables ([`parse::env_proxy`], [`NoProxy`]). Which variable a program reads is the caller's
//! (it differs per agent); this file only follows the values.
//!
//! [`Upstream::System`]: super::Upstream::System

use url::Url;

use super::Route;
use super::parse::{self, NoProxy, normal_host};

/// A launch's proxy variable that an `http://` `CONNECT` cannot follow: an `https://` proxy,
/// another scheme (`socks5://`), no host, or not a URL. It never holds the value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnusableProxy;

/// The route to `target` of a program launched with `proxy` (the proxy variable it reads, as
/// set: `None`, empty or blank for none) and `no_proxy` (its `NO_PROXY` list): straight to the
/// host when no proxy is set or the list holds the host; else the proxy, read as curl reads
/// `HTTPS_PROXY` (no scheme is `http://`, a user name or password is dropped, a path ignored).
/// A proxy that is set but cannot be followed is [`UnusableProxy`], never a guess at direct.
pub fn launch_route(
    proxy: Option<&str>,
    no_proxy: Option<&str>,
    target: &Url,
) -> Result<Route, UnusableProxy> {
    let Some(proxy) = proxy.filter(|proxy| !proxy.trim().is_empty()) else {
        return Ok(Route::Direct);
    };
    let (Some(host), Some(port)) = (target.host_str(), target.port_or_known_default()) else {
        return Ok(Route::Direct);
    };
    if NoProxy::parse(no_proxy.unwrap_or_default()).matches(&normal_host(host), port) {
        return Ok(Route::Direct);
    }
    parse::env_proxy(proxy)
        .map(|proxy| proxy.route)
        .map_err(|_bad| UnusableProxy)
}

#[cfg(test)]
mod tests {
    use url::Url;

    use super::{Route, UnusableProxy, launch_route};

    fn target() -> Url {
        Url::parse("https://api.anthropic.com/v1").unwrap()
    }

    fn firm() -> Route {
        Route::Http {
            host: "proxy.firm.example".to_owned(),
            port: 3128,
        }
    }

    #[test]
    fn no_proxy_set_goes_straight() {
        for proxy in [None, Some(""), Some("  ")] {
            assert_eq!(launch_route(proxy, None, &target()), Ok(Route::Direct));
        }
    }

    #[test]
    fn a_set_proxy_is_the_route_its_credentials_dropped() {
        let proxy = Some("http://user:secret@proxy.firm.example:3128");
        assert_eq!(launch_route(proxy, None, &target()), Ok(firm()));
        let bare = Some("proxy.firm.example:3128");
        assert_eq!(launch_route(bare, Some(""), &target()), Ok(firm()));
    }

    #[test]
    fn no_proxy_holding_the_host_goes_straight_as_the_agent_would() {
        let proxy = Some("http://proxy.firm.example:3128");
        for list in [
            "localhost,127.0.0.1,::1,anthropic.com",
            ".anthropic.com",
            "API.Anthropic.com:443",
            "*",
        ] {
            let route = launch_route(proxy, Some(list), &target());
            assert_eq!(route, Ok(Route::Direct), "{list}");
        }
        let other = Some("localhost,127.0.0.1,::1,api.anthropic.com:8443");
        assert_eq!(launch_route(proxy, other, &target()), Ok(firm()));
    }

    #[test]
    fn a_proxy_a_connect_cannot_follow_is_unusable_never_direct() {
        for proxy in [
            "https://proxy.firm.example:3128",
            "socks5://proxy.firm.example:1080",
            "http://",
        ] {
            let route = launch_route(Some(proxy), None, &target());
            assert_eq!(route, Err(UnusableProxy), "{proxy}");
        }
    }
}
