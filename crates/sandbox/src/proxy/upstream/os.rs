//! The OS part of the upstream lookup: the URL the OS is asked about, the macOS and Windows
//! calls, and what a failure's settings show.

#[cfg(windows)]
use std::time::Duration;

#[cfg(any(target_os = "macos", windows, test))]
use url::Url;

#[cfg(any(target_os = "macos", windows, test))]
use super::UpstreamAnswer;
#[cfg(windows)]
use super::bounded::bounded;
#[cfg(target_os = "macos")]
use super::macos;
#[cfg(windows)]
use super::windows;
use super::{Answer, Failure, Route, Scheme};
#[cfg(windows)]
use super::{Configured, OS_LOOKUPS_MOST};

/// The `https://host:port/` or `http://host:port/` the OS is asked about, or `None` for a host
/// no URL can hold.
#[cfg(any(target_os = "macos", windows, test))]
pub(super) fn target_url(scheme: Scheme, host: &str, port: u16) -> Option<String> {
    let authority = if host.contains(':') {
        format!("[{host}]")
    } else {
        host.to_owned()
    };
    let scheme = match scheme {
        Scheme::Https => "https",
        Scheme::Http => "http",
    };
    let url = Url::parse(&format!("{scheme}://{authority}:{port}/")).ok()?;
    url.host_str().is_some().then(|| url.into())
}

/// An OS answer, or `Failed` when the lookup failed, logged by port only: the host is where the
/// agent goes, which the log never holds (decision record 0047).
#[cfg(any(target_os = "macos", windows, test))]
pub(super) fn settle(port: u16, answer: Result<Route, String>) -> UpstreamAnswer {
    match answer {
        Ok(route) => route.into(),
        Err(why) => {
            tracing::warn!(port, why, "the OS proxy lookup failed");
            UpstreamAnswer::Failed
        }
    }
}

/// [`settle`], a failure with `failure()`: whether the settings show a proxy meant.
#[cfg(any(target_os = "macos", windows))]
pub(super) fn settle_with(
    port: u16,
    answer: Result<Route, String>,
    failure: impl FnOnce() -> Failure,
) -> Answer {
    match settle(port, answer) {
        UpstreamAnswer::Route(route) => route.into(),
        UpstreamAnswer::Failed => Answer::Failed(failure()),
    }
}

/// The OS lookup: `CFNetwork`'s proxies for the target URL.
#[cfg(target_os = "macos")]
pub(super) fn os_route(scheme: Scheme, host: &str, port: u16) -> Answer {
    let answer = target_url(scheme, host, port)
        .ok_or_else(|| "the host is not a URL host".to_owned())
        .and_then(|target| macos::route(&target));
    settle_with(port, answer, os_intent)
}

/// The OS lookup: `WinHTTP`'s proxy for the target URL, the whole call bounded to
/// [`WINDOWS_LOOKUP_LIMIT`]: WPAD's DHCP and DNS discovery do not all honour `WinHTTP`'s
/// timeouts. A failure past the bound reads the settings again for its [`Failure`].
#[cfg(windows)]
pub(super) fn os_route(scheme: Scheme, host: &str, port: u16) -> Answer {
    let answer = target_url(scheme, host, port)
        .ok_or_else(|| "the host is not a URL host".to_owned())
        .and_then(|target| {
            let host = host.to_owned();
            let threads = &WINDOWS_LOOKUP_THREADS;
            bounded(
                WINDOWS_LOOKUP_LIMIT,
                threads,
                WINDOWS_LOOKUP_THREADS_MOST,
                move || Ok(windows::route(&target, &host, port)),
            )
        });
    let mut configured = None;
    let answer = match answer {
        Ok(Ok(route)) => Ok(route),
        Ok(Err(failed)) => {
            configured = failed.configured;
            Err(failed.why)
        }
        Err(why) => Err(why),
    };
    settle_with(port, answer, || {
        configured.map_or_else(os_intent, Configured::failure)
    })
}

/// Whether the OS settings show the firm meant a proxy: a PAC URL or a static proxy.
#[cfg(target_os = "macos")]
pub(super) fn os_intent() -> Failure {
    macos::configured().failure()
}

/// Whether the OS settings show the firm meant a proxy: a PAC URL or a static proxy.
#[cfg(windows)]
pub(super) fn os_intent() -> Failure {
    windows::configured().failure()
}

/// No OS settings on Linux and others.
#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn os_intent() -> Failure {
    Failure::Unmeant
}

/// The longest a Windows lookup (WPAD discovery and the PAC run included) may take.
#[cfg(windows)]
const WINDOWS_LOOKUP_LIMIT: Duration = Duration::from_secs(10);

/// Windows lookup threads alive now, those past their bound included.
#[cfg(windows)]
static WINDOWS_LOOKUP_THREADS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// The most Windows lookup threads alive at once, across the process: as many as
/// [`OS_LOOKUPS_MOST`], so a lookup that got a slot there is not failed (and cached as failed)
/// here for want of a thread.
#[cfg(windows)]
const WINDOWS_LOOKUP_THREADS_MOST: usize = OS_LOOKUPS_MOST;

/// The OS lookup on Linux and others: none, so direct.
#[cfg(not(any(target_os = "macos", windows)))]
pub(super) fn os_route(_scheme: Scheme, _host: &str, _port: u16) -> Answer {
    Route::Direct.into()
}
