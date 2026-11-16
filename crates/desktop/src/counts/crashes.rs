//! Crash reports' desktop half (decision record 0060). The reports, their queue and their
//! sending are `clauseo-counts`' `Crashes`; the panic hook and the native handler are the host's
//! (`clauseo_host::crash`). Here:
//!
//! - Opening ([`open`]), with the counts on the host's start thread: the last run's crashes are
//!   taken in (each adds `crash` to the tally), then [`clauseo_host::crash::record_crashes`] has
//!   this run's recorded. A crash before that point (the settings read, the launch profile) is
//!   not recorded.
//! - The webview's renderer gone, through the hook each OS gives: macOS's
//!   `webViewWebContentProcessDidTerminate:` ([`on_renderer_gone`], Tauri's builder hook; the
//!   page is loaded again, as Tauri does with no hook set), Windows' `WebView2`
//!   `ProcessFailed` for a render process that exited, and Linux's `WebKitGTK`
//!   `web-process-terminated` ([`watch_renderer`]). An end the app or something outside asked
//!   for (`WebView2`'s `TERMINATED`, `WebKitGTK`'s `TERMINATED_BY_API`) is not a crash and makes
//!   no report; macOS gives no reason, so its reports say `other`.

use clauseo_counts::schema::RendererGoneReason;
use clauseo_counts::{About, Counts, Crashes, Switch};
use clauseo_settings::{Effective, Locations};
use tauri::{AppHandle, Manager as _, Runtime};

#[cfg(windows)]
mod win;

/// The settings key whose policy lock keeps crash reports from being sent.
const CRASH_REPORTS_KEY: &str = "privacy.crash_reports";

/// Opens the crash reports of `locations`' data folder beside `counts`, takes in the last run's
/// crashes, and starts recording this run's.
pub(crate) fn open(
    locations: &Locations,
    effective: &Effective,
    about: About,
    counts: &Counts,
) -> Crashes {
    let crashes = Crashes::open(locations.data_dir(), about, clauseo_counts::os_seed());
    let switch = Switch {
        counts: super::counts_on(effective),
        locked_off: effective.is_locked(CRASH_REPORTS_KEY),
    };
    if let Err(err) = crashes.ingest(counts, switch, super::now()) {
        // The kind alone: the error's text could hold a path.
        tracing::debug!(kind = %err.kind(), "the crash reports were not all taken in; the next tick tries again");
    }
    let native = clauseo_host::crash::record_crashes(&crashes);
    tracing::debug!(native, "crash reports started");
    crashes
}

/// The renderer of one of `app`'s webviews went, for `reason`.
fn renderer_gone<R: Runtime>(app: &AppHandle<R>, reason: RendererGoneReason) {
    if let Some(state) = app.try_state::<super::CountsState>() {
        state.renderer_gone(reason);
    }
}

/// macOS: records each `WebContent` process that ends, then loads the page again.
#[cfg(target_os = "macos")]
pub(crate) fn on_renderer_gone<R: Runtime>(builder: tauri::Builder<R>) -> tauri::Builder<R> {
    builder.on_web_content_process_terminate(|webview| {
        renderer_gone(webview.app_handle(), RendererGoneReason::Other);
        // What Tauri does when no hook is set.
        if let Err(err) = webview.reload() {
            tracing::warn!(%err, "the page did not load again after its WebContent process ended");
        }
    })
}

/// Windows and Linux: watches `window`'s renderer.
#[cfg(any(windows, target_os = "linux"))]
pub(crate) fn watch_renderer<R: Runtime>(window: &tauri::WebviewWindow<R>) {
    let app = window.app_handle().clone();
    if let Err(err) = window.with_webview(move |webview| watch(&webview, app)) {
        tracing::debug!(%err, "the webview's renderer is not watched for crashes");
    }
}

/// Elsewhere (macOS) the builder's hook watches every webview.
#[cfg(not(any(windows, target_os = "linux")))]
pub(crate) fn watch_renderer<R: Runtime>(_window: &tauri::WebviewWindow<R>) {}

#[cfg(windows)]
fn watch<R: Runtime>(webview: &tauri::webview::PlatformWebview, app: AppHandle<R>) {
    let watched = win::watch(&webview.controller(), move |reason| {
        renderer_gone(&app, reason);
    });
    if let Err(err) = watched {
        tracing::debug!(%err, "WebView2's ProcessFailed is not watched");
    }
}

#[cfg(target_os = "linux")]
fn watch<R: Runtime>(webview: &tauri::webview::PlatformWebview, app: AppHandle<R>) {
    use webkit2gtk::{WebProcessTerminationReason, WebViewExt as _};
    let _handler = webview
        .inner()
        .connect_web_process_terminated(move |_view, reason| {
            let reason = match reason {
                WebProcessTerminationReason::Crashed => RendererGoneReason::Crashed,
                WebProcessTerminationReason::ExceededMemoryLimit => RendererGoneReason::OutOfMemory,
                // The app asked for it: not a crash.
                WebProcessTerminationReason::TerminatedByApi => return,
                _ => RendererGoneReason::Other,
            };
            renderer_gone(&app, reason);
        });
}
