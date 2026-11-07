//! What a lookup answers: the public [`UpstreamAnswer`], whose failure has a [`FailKind`], and
//! inside the crate an [`Answer`] whose failure also says whether the settings show the firm
//! meant a proxy, for the egress proxy's choice over a failed lookup (decision record 0039, "A
//! failed lookup").

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
    /// No answer from the OS, and why.
    Failed(FailKind),
}

/// Why an OS lookup gave no answer. Later kinds are added at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailKind {
    /// The PAC file the settings name could not be fetched: `WinHTTP`'s 12167
    /// (`ERROR_WINHTTP_UNABLE_TO_DOWNLOAD_SCRIPT`) for a PAC URL, its host not answering or
    /// timing out, or a macOS PAC fetch that failed. Usually the laptop is off the firm's
    /// network. Never from a static proxy setting.
    PacUnreachable,
    /// The PAC script was got and would not run: `WinHTTP`'s 12166
    /// (`ERROR_WINHTTP_BAD_AUTO_PROXY_SCRIPT`), or a macOS PAC script that fails to evaluate.
    BadScript,
    /// Anything else: no free lookup slot, settings unreadable or too slow, and every other
    /// error.
    Other,
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
            UpstreamAnswer::Failed(_) => Route::Direct,
        }
    }
}

/// Whether the proxy settings show the firm meant its traffic to go through a proxy, as known
/// when a lookup failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Failure {
    /// Nothing shows it: the settings were read and name no PAC URL, PAC script or static
    /// proxy, and no PAC script failed to run (a plain network, or WPAD with nothing found).
    #[default]
    Unmeant,
    /// A PAC URL, a PAC script or a static proxy is set, `WinHTTP` found a PAC script it could
    /// not run, or the settings could not be read (or not in time): refusing is safer than going
    /// around a proxy the firm may require.
    MeantProxy,
}

/// A PAC file the settings name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg(any(target_os = "macos", windows, test))]
pub(crate) enum PacSetting {
    /// None.
    #[default]
    None,
    /// A PAC URL, or a script held inline (macOS `ProxyAutoConfigJavaScript`, as an MDM profile
    /// pushes).
    Named,
}

/// What a failed lookup's settings held, for [`Configured::failure`].
#[derive(Debug, Clone, Copy, Default)]
#[cfg(any(target_os = "macos", windows, test))]
pub(crate) struct Configured {
    /// The PAC file the settings name, if any.
    pub(crate) pac: PacSetting,
    /// The settings name a static HTTP or HTTPS proxy.
    pub(crate) static_proxy: bool,
    /// The lookup failed with `ERROR_WINHTTP_BAD_AUTO_PROXY_SCRIPT` (12166): a PAC script was
    /// found, WPAD's included, and would not run.
    pub(crate) broken_script: bool,
    /// The settings could not be read.
    pub(crate) unreadable: bool,
}

#[cfg(any(target_os = "macos", windows, test))]
impl Configured {
    /// Any of them means the firm meant a proxy, or may have.
    pub(crate) fn failure(self) -> Failure {
        if self.pac != PacSetting::None
            || self.static_proxy
            || self.broken_script
            || self.unreadable
        {
            Failure::MeantProxy
        } else {
            Failure::Unmeant
        }
    }
}

/// [`UpstreamAnswer`] with the failure's [`Failure`] beside its kind: what the OS lookup gives
/// and its cache keeps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Answer {
    /// The route.
    Route(Route),
    /// No answer from the OS: whether a proxy was meant, and why.
    Failed(Failure, FailKind),
}

impl From<Route> for Answer {
    fn from(route: Route) -> Self {
        Answer::Route(route)
    }
}

/// A public `Failed` carries no settings: nothing shows the firm meant a proxy.
impl From<UpstreamAnswer> for Answer {
    fn from(answer: UpstreamAnswer) -> Self {
        match answer {
            UpstreamAnswer::Route(route) => Answer::Route(route),
            UpstreamAnswer::Failed(kind) => Answer::Failed(Failure::Unmeant, kind),
        }
    }
}

impl From<Answer> for UpstreamAnswer {
    fn from(answer: Answer) -> Self {
        match answer {
            Answer::Route(route) => UpstreamAnswer::Route(route),
            Answer::Failed(_, kind) => UpstreamAnswer::Failed(kind),
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
