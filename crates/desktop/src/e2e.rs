//! The `e2e` feature: what the end-to-end tests need that a person would do by hand.
//!
//! - On Windows the webview (`WebView2`) opens its remote debugging port on 127.0.0.1 so
//!   Playwright can attach with `connectOverCDP`. The port is 0, so the browser binds a free port
//!   itself and writes it on the first line of `EBWebView/DevToolsActivePort` in its user data
//!   folder (`WEBVIEW2_USER_DATA_FOLDER`), where the harness reads it. The file appears once the
//!   browser listens, after the window is built; the harness polls for it.
//! - On every OS, `pick_folder` returns the folder in `E2E_MATTER_DIR` instead of drawing the
//!   folder dialog, and the "always allow" confirm answers `E2E_CONFIRM_ALWAYS` (`accept` or
//!   `decline`) instead of drawing its dialog. Either variable unset or empty (any other value,
//!   for the confirm) draws the dialog as usual, for runs by hand.
//! - The speed scoreboard's measures (docs/decisions/0012-desktop-bridge.md):
//!   - each channel event that fits one message wrapped goes as
//!     `{"timed":{"received_ms":<f64>,"event":<event>}}`, the time the bridge received it from
//!     the host subscription (`frame::timed_messages`); a larger one goes as in release;
//!   - an initialization script, run in the window before any page script, sets
//!     `window.__CLAUSEO_PERF__ = { t0EpochMs, autoplay, reactCommits: 0 }` (`t0EpochMs` the
//!     app's start, read on the first line of `run()`; `autoplay` true only with `marks`) and,
//!     unless the page has one, a `__REACT_DEVTOOLS_GLOBAL_HOOK__` whose `onCommitFiberRoot`
//!     counts React commits into `reactCommits`;
//!   - the command `perf_channel_bytes`: the bytes sent down every window channel since start.
//! - The `marks` feature (`marks.rs`) adds to all of this `marks_memory` and `marks_finish`.
//!
//! Contract:
//! - Built only with `--features e2e` (or `marks`, which turns it on). The default build has
//!   no `e2e` feature, so a release never opens a debugging port, never reads these variables,
//!   sends no `timed` message, runs no initialization script and has no perf command:
//!   `build.rs` defines its permission and `grant_scoreboard` adds `capabilities/e2e.json` at
//!   setup, in this build only.
//! - `E2E_DISABLE_GPU=1` adds `--disable-gpu`, for the second CI run (Windows).
//! - Once the host has stopped and joined, the app ends with `std::process::exit` instead of
//!   `app.exit` (`lib.rs`, `stop_host_then_exit`): in a Windows session with no visible window,
//!   `app.exit` never returns from the event loop. So window-state never saves in this build.

use std::path::PathBuf;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU64, Ordering};

use clauseo_protocol::HostError;
use tauri::{AppHandle, Manager, Runtime};

/// The app's start in ms since the Unix epoch, set on the first line of `run()`.
static T0_EPOCH_MS: OnceLock<f64> = OnceLock::new();

/// The bytes sent down every window channel since the app started.
static CHANNEL_BYTES: AtomicU64 = AtomicU64::new(0);

/// The time now in ms since the Unix epoch, with its fraction; 0 for a clock before 1970.
#[expect(
    clippy::disallowed_methods,
    reason = "e2e build only: the scoreboard's wall-clock stamps, compared with the page's clock"
)]
pub(crate) fn epoch_ms() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0.0, |since| since.as_secs_f64() * 1000.0)
}

/// Records the app's start. Called first in `run()`; a later call changes nothing.
pub(crate) fn mark_start() {
    T0_EPOCH_MS.get_or_init(epoch_ms);
}

/// Adds `len` bytes, one channel message sent, to the total.
pub(crate) fn count_channel_bytes(len: usize) {
    CHANNEL_BYTES.fetch_add(len as u64, Ordering::Relaxed);
}

/// The window's initialization script: the page's perf globals, set before any page script.
pub(crate) fn init_script() -> String {
    init_script_with(
        T0_EPOCH_MS.get().copied().unwrap_or_else(epoch_ms),
        cfg!(feature = "marks"),
    )
}

/// [`init_script`] for a start at `t0_epoch_ms` and the given `autoplay`.
fn init_script_with(t0_epoch_ms: f64, autoplay: bool) -> String {
    // A time that is not finite (never: the clock reads a duration) would not be valid JS.
    let t0 = if t0_epoch_ms.is_finite() {
        t0_epoch_ms
    } else {
        0.0
    };
    format!(
        r"window.__CLAUSEO_PERF__ = {{ t0EpochMs: {t0}, autoplay: {autoplay}, reactCommits: 0 }};
if (window.__REACT_DEVTOOLS_GLOBAL_HOOK__ === undefined) {{
  window.__REACT_DEVTOOLS_GLOBAL_HOOK__ = {{
    supportsFiber: true, renderers: new Map(), inject() {{ return 1; }},
    onCommitFiberRoot() {{ window.__CLAUSEO_PERF__.reactCommits++; }},
    onCommitFiberUnmount() {{}}, onPostCommitFiberRoot() {{}}, checkDCE() {{}},
  }};
}}
"
    )
}

/// Grants the main window the scoreboard's commands: `capabilities/e2e.json`, and with `marks`
/// `capabilities/marks.json`. They are added here, at setup, because `tauri.conf.json` names
/// only `main` and a context with more capabilities would need `generate_context!` arguments,
/// which the tauri-config check refuses. The files stay in `capabilities/`, where the
/// capabilities check reads them; the permissions they grant exist only in these builds
/// (`build.rs`).
#[expect(
    clippy::disallowed_methods,
    reason = "e2e/marks builds only; release ACL unchanged: grants the scoreboard's capability files"
)]
pub(crate) fn grant_scoreboard<R: Runtime, M: Manager<R>>(manager: &M) -> tauri::Result<()> {
    manager.add_capability(include_str!("../capabilities/e2e.json"))?;
    #[cfg(feature = "marks")]
    manager.add_capability(include_str!("../capabilities/marks.json"))?;
    Ok(())
}

/// The bytes the bridge has sent down every window channel since the app started: the sum of
/// the message lengths it counts for flow control.
#[tauri::command]
pub(crate) async fn perf_channel_bytes<R: Runtime>(_app: AppHandle<R>) -> Result<u64, HostError> {
    Ok(channel_bytes())
}

/// The bytes sent down every window channel since the app started.
pub(crate) fn channel_bytes() -> u64 {
    CHANNEL_BYTES.load(Ordering::Relaxed)
}

/// The folder `pick_folder` returns instead of drawing the dialog.
const MATTER_DIR_VAR: &str = "E2E_MATTER_DIR";

/// `accept` or `decline`: the answer the "always allow" confirm gives without a dialog.
const CONFIRM_ALWAYS_VAR: &str = "E2E_CONFIRM_ALWAYS";

/// The folder in `E2E_MATTER_DIR`, when it is set and not empty.
pub(crate) fn matter_dir() -> Option<PathBuf> {
    std::env::var_os(MATTER_DIR_VAR)
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
}

/// The confirm's answer from `E2E_CONFIRM_ALWAYS`: `Some(true)` for `accept`, `Some(false)`
/// for `decline`, `None` (draw the dialog) otherwise.
pub(crate) fn confirm_always() -> Option<bool> {
    match std::env::var(CONFIRM_ALWAYS_VAR).ok()?.as_str() {
        "accept" => Some(true),
        "decline" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests;

#[cfg(windows)]
pub(crate) use debugging::with_debugging_port;

#[cfg(windows)]
mod debugging {
    use tauri::{Manager, Runtime, WebviewWindowBuilder};

    /// wry's own browser arguments. Passing `additional_browser_args` replaces them (wry
    /// `src/webview2/mod.rs`), so they are passed again here.
    const WRY_DEFAULT_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

    /// Set to `1` to run the webview without the GPU.
    const DISABLE_GPU_VAR: &str = "E2E_DISABLE_GPU";

    /// `builder` with the webview's remote debugging port on a free port of 127.0.0.1, which
    /// the browser picks and writes to `DevToolsActivePort`.
    pub(crate) fn with_debugging_port<R: Runtime, M: Manager<R>>(
        builder: WebviewWindowBuilder<'_, R, M>,
    ) -> WebviewWindowBuilder<'_, R, M> {
        let mut args = format!("{WRY_DEFAULT_ARGS} --remote-debugging-port=0");
        if std::env::var(DISABLE_GPU_VAR).is_ok_and(|value| value == "1") {
            args.push_str(" --disable-gpu");
        }
        builder.additional_browser_args(&args)
    }
}
