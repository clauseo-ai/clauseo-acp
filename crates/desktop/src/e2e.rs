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
//!
//! Contract:
//! - Built only with `--features e2e`. The default build has no `e2e` feature, so a release
//!   never opens a debugging port and never reads these variables.
//! - `E2E_DISABLE_GPU=1` adds `--disable-gpu`, for the second CI run (Windows).
//! - Once the host has stopped and joined, the app ends with `std::process::exit` instead of
//!   `app.exit` (`lib.rs`, `stop_host_then_exit`): in a Windows session with no visible window,
//!   `app.exit` never returns from the event loop. So window-state never saves in this build.

use std::path::PathBuf;

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
