//! The start-up's marks in host.log, so a slow start can be split into its phases from the log
//! alone, with no ETW: one info line per step, each with `at_ms`, the milliseconds since `run`
//! began, on the chooser's clock (`chooser/trace.rs`, which the host start's own marks and each
//! command's `ipc::lifecycle` line use too). Never a path, a URL or any of the lawyer's data.
//!
//! - [`TARGET`] (`clauseo_desktop::startup::lifecycle`): `run` entered (logged once the log is up,
//!   at its own `at_ms`, 0), the `WebView2` runtime checked, the main window's build started and
//!   done (on Windows the done line comes after `WebView2`'s environment and controller exist), the
//!   host thread spawned, the event loop ready (`RunEvent::Ready`), the first response the app's
//!   protocol built (`index_html` says whether it was the page; the hook runs on Tauri's async
//!   runtime as the response is ready, before `WebView2` gets it back on the main thread), and
//!   the main window's page load started and finished.
//! - [`PAGE_TARGET`] (`clauseo_desktop::page::lifecycle`): the page's own marks
//!   (`ui/app/src/start-marks.ts`), one line from `page_marks`, the first call of a run only. The
//!   page sends each mark as how long before the call it was; the line puts it on the same clock,
//!   so it reads a few ms late by the call's own transit.

use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_settings::{SystemDirs, SystemEnv};

use crate::chooser::trace::at_ms;
use crate::host_log::HostLog;

/// The target of the desktop's start-up lines: host.log keeps info only from
/// `clauseo_*::lifecycle` targets (`clauseo_host::log_file`).
pub(crate) const TARGET: &str = "clauseo_desktop::startup::lifecycle";

/// The target of the page's start-up line.
pub(crate) const PAGE_TARGET: &str = "clauseo_desktop::page::lifecycle";

/// The start's first steps: checks the `WebView2` runtime (`webview2.rs`: on Windows, no window
/// without one that can draw it), then starts the host's log file (decision record 0047,
/// `host_log.rs`, its guard managed with the app and flushed when the host stops) and logs the
/// marks taken before it was up: `run` entered, at 0, and the runtime checked. `None` when the
/// app must not start.
pub(crate) fn check_then_log() -> Option<HostLog> {
    if crate::webview2::stops_start() {
        return None;
    }
    let checked = at_ms();
    let log = crate::host_log::install(&SystemDirs, &SystemEnv);
    mark_at("run entered", 0);
    mark_at("WebView2 runtime checked", checked);
    Some(log)
}

/// Logs the start-up's step `what`, now.
pub(crate) fn mark(what: &str) {
    mark_at(what, at_ms());
}

/// Logs the start-up's step `what`, which happened at `at` ms since `run` began (a step taken
/// before the log was up).
pub(crate) fn mark_at(what: &str, at: u64) {
    tracing::info!(target: TARGET, at_ms = at, "start-up: {what}");
}

/// Whether this run marked the first response of the app's protocol.
static FIRST_RESPONSE: AtomicBool = AtomicBool::new(false);

/// The start-up's mark for the first response the app's protocol built, once a run; `path` is
/// the request's, read only to say whether it was `index.html`, never logged.
pub(crate) fn first_response(path: &str) {
    if FIRST_RESPONSE.swap(true, Ordering::Relaxed) {
        return;
    }
    let index_html = is_page(path);
    tracing::info!(
        target: TARGET,
        at_ms = at_ms(),
        index_html,
        "start-up: first app response built"
    );
}

/// Whether a request's `path` is the room's page: `index.html`, or the root Tauri answers with it.
pub(crate) fn is_page(path: &str) -> bool {
    matches!(path, "" | "/" | "/index.html")
}

/// The start-up's mark for the run loop's events: `RunEvent::Ready`, the event loop up.
pub(crate) fn on_run_event(event: &tauri::RunEvent) {
    if matches!(event, tauri::RunEvent::Ready) {
        mark("event loop ready");
    }
}

/// The page's start-up marks, as `ui/app/src/start-marks.ts` takes them: each is its age, the ms
/// from it to the call leaving the page; `None` for a mark it did not take.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct PageMarks {
    /// The page's time origin.
    pub(crate) navigation: u32,
    /// The page's first module running.
    pub(crate) script: Option<u32>,
    /// The root's render called.
    pub(crate) render: Option<u32>,
    /// The welcome's first commit.
    pub(crate) welcome: Option<u32>,
    /// The welcome's primary button mounted.
    pub(crate) button: Option<u32>,
}

/// Each mark of `marks` on the desktop's clock, given the call reached it at `now` ms since
/// `run`: `now` less how long before the call the mark was, at least 0. `None` stays `None`.
pub(crate) fn on_clock(marks: PageMarks, now: u64) -> [Option<u64>; 5] {
    let at = |ago: u32| now.saturating_sub(u64::from(ago));
    [
        Some(at(marks.navigation)),
        marks.script.map(at),
        marks.render.map(at),
        marks.welcome.map(at),
        marks.button.map(at),
    ]
}

/// Whether this run logged the page's line: one per run, so script in the page can't flood
/// host.log.
static PAGE_LOGGED: AtomicBool = AtomicBool::new(false);

/// Logs the page's marks on the desktop's clock, the first call of a run only; `-1` for a mark
/// the page did not take.
pub(crate) fn log_page(marks: PageMarks) {
    if PAGE_LOGGED.swap(true, Ordering::Relaxed) {
        return;
    }
    let [navigation, script, render, welcome, button] = on_clock(marks, at_ms());
    let ms = |at: Option<u64>| at.map_or(-1, |at| i64::try_from(at).unwrap_or(i64::MAX));
    tracing::info!(
        target: PAGE_TARGET,
        navigation_ms = ms(navigation),
        script_ms = ms(script),
        render_ms = ms(render),
        welcome_ms = ms(welcome),
        button_ms = ms(button),
        "start-up: page marks"
    );
}

#[cfg(test)]
mod tests;
