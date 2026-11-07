//! What a lookup answers: the public [`UpstreamAnswer`], and inside the crate an [`Answer`] whose
//! failure says whether the settings show the firm meant a proxy, for the egress proxy's choice
//! over a failed lookup (decision record 0039, "A failed lookup").

use std::sync::OnceLock;
use std::time::Duration;

use url::Url;

use super::{Env, Lookup, Route, Scheme, Source, System, os_route};
use crate::{Error, SystemClock};

/// An upstream lookup's answer for [`upstream_answer`]: a route, or `Failed` when the OS was
/// asked and gave no answer (the lookup failed or timed out, or every lookup slot was busy).
/// [`UpstreamAnswer::route`] takes a failure as [`Route::Direct`]; a caller that keeps an earlier
/// answer (desktop's refreshed route) keeps it over a failure. The egress proxy's own connections
/// decide over a failure by decision record 0039 instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpstreamAnswer {
    /// The settings', the environment's or the OS's route, `DIRECT` included.
    Route(Route),
    /// No answer from the OS.
    Failed,
}

impl From<Route> for UpstreamAnswer {
    fn from(route: Route) -> Self {
        UpstreamAnswer::Route(route)
    }
}

impl UpstreamAnswer {
    /// The route a connection takes: a failure goes direct.
    pub fn route(self) -> Route {
        match self {
            UpstreamAnswer::Route(route) => route,
            UpstreamAnswer::Failed => Route::Direct,
        }
    }
}

/// Whether the proxy settings show the firm meant its traffic to go through a proxy, as known
/// when a lookup failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Failure {
    /// Nothing shows it: no PAC URL, no static proxy, no broken PAC script (a plain network,
    /// or WPAD with nothing found, or settings that could not be read).
    #[default]
    Unmeant,
    /// A PAC URL or a static proxy is set, or `WinHTTP` found a PAC script it could not run.
    MeantProxy,
}

/// What a failed lookup's settings held, for [`Configured::failure`].
#[derive(Debug, Clone, Copy, Default)]
#[cfg(any(target_os = "macos", windows, test))]
pub(crate) struct Configured {
    /// The settings name a PAC URL.
    pub(crate) pac_url: bool,
    /// The settings name a static HTTP or HTTPS proxy.
    pub(crate) static_proxy: bool,
    /// The lookup failed with `ERROR_WINHTTP_BAD_AUTO_PROXY_SCRIPT` (12166): a PAC script was
    /// found, WPAD's included, and would not run.
    pub(crate) broken_script: bool,
}

#[cfg(any(target_os = "macos", windows, test))]
impl Configured {
    /// Any of the three means the firm meant a proxy.
    pub(crate) fn failure(self) -> Failure {
        if self.pac_url || self.static_proxy || self.broken_script {
            Failure::MeantProxy
        } else {
            Failure::Unmeant
        }
    }
}

/// [`UpstreamAnswer`] with the failure's [`Failure`]: what the OS lookup gives and its cache
/// keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Answer {
    /// The route.
    Route(Route),
    /// No answer from the OS.
    Failed(Failure),
}

impl From<Route> for Answer {
    fn from(route: Route) -> Self {
        Answer::Route(route)
    }
}

/// A public `Failed` carries no reason: nothing shows the firm meant a proxy.
impl From<UpstreamAnswer> for Answer {
    fn from(answer: UpstreamAnswer) -> Self {
        match answer {
            UpstreamAnswer::Route(route) => Answer::Route(route),
            UpstreamAnswer::Failed => Answer::Failed(Failure::Unmeant),
        }
    }
}

impl From<Answer> for UpstreamAnswer {
    fn from(answer: Answer) -> Self {
        match answer {
            Answer::Route(route) => UpstreamAnswer::Route(route),
            Answer::Failed(_) => UpstreamAnswer::Failed,
        }
    }
}

/// [`Lookup::decide`]'s answer, with the times on the lookup's clock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Decided {
    pub(crate) answer: Answer,
    /// For a route the OS gave, when a real lookup gave it (a cached answer keeps its time);
    /// `None` for a route from the settings or the environment, and for a failure.
    pub(crate) got: Option<Duration>,
    /// The lookup clock's time when it answered (zero when the lookup has no OS part).
    pub(crate) now: Duration,
}

impl Decided {
    /// A route not from the OS.
    pub(super) fn plain(route: Route) -> Decided {
        Decided {
            answer: route.into(),
            got: None,
            now: Duration::ZERO,
        }
    }
}

/// The upstream for the host's own HTTPS (or HTTP) outside the jail (stage E's connector relay).
/// Blocking, like [`Lookup::route`]. [`Upstream::System`] shares one process-wide [`Lookup`]
/// and its cache, its environment read at the first call. A [`Upstream::Fixed`] URL that
/// [`start_proxy`](crate::start_proxy) would refuse is refused here too
/// (`Error::ProxyConfig`), so the connection fails where the lawyer sees it rather than going
/// around the proxy the firm requires.
///
/// [`Upstream::System`]: super::Upstream::System
/// [`Upstream::Fixed`]: super::Upstream::Fixed
pub fn upstream_for(upstream: &super::Upstream, url: &Url) -> Result<Route, Error> {
    upstream_answer(upstream, url).map(UpstreamAnswer::route)
}

/// [`upstream_for`], a failed OS lookup (failed, timed out, or no slot) kept apart from an
/// explicit `DIRECT` as [`UpstreamAnswer::Failed`]. Same cache: a failure is kept 60 s too.
pub fn upstream_answer(upstream: &super::Upstream, url: &Url) -> Result<UpstreamAnswer, Error> {
    static SYSTEM: OnceLock<Lookup> = OnceLock::new();
    let made;
    let lookup = if let super::Upstream::System = upstream {
        SYSTEM.get_or_init(|| Lookup {
            source: Source::System(Box::new(System::new(
                Env::from_process(),
                Box::new(os_route),
                Box::new(SystemClock::new()),
            ))),
        })
    } else {
        made = Lookup::new(upstream.clone())?;
        &made
    };
    let Some(host) = url.host_str() else {
        return Ok(Route::Direct.into());
    };
    let scheme = Scheme::of(url);
    let port = url.port_or_known_default().unwrap_or(match scheme {
        Scheme::Https => 443,
        Scheme::Http => 80,
    });
    Ok(lookup.answer(scheme, host, port))
}
