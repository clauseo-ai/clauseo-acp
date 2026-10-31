//! The `e2e` feature, Windows only: the webview (`WebView2`) opens its remote debugging port on
//! 127.0.0.1 so Playwright can attach with `connectOverCDP`. The port is 0, so the browser binds a
//! free port itself and writes it on the first line of `EBWebView/DevToolsActivePort` in its user
//! data folder (`WEBVIEW2_USER_DATA_FOLDER`), where the harness reads it.
//!
//! Contract:
//! - Built only with `--features e2e` on Windows. The default build has no `e2e` feature, so a
//!   release never opens a debugging port.
//! - `E2E_DISABLE_GPU=1` adds `--disable-gpu`, for the second CI run.
//! - The file appears once the browser listens, after the window is built; the harness polls
//!   for it.

use tauri::{Manager, Runtime, WebviewWindowBuilder};

/// wry's own browser arguments. Passing `additional_browser_args` replaces them (wry
/// `src/webview2/mod.rs`), so they are passed again here.
const WRY_DEFAULT_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Set to `1` to run the webview without the GPU.
const DISABLE_GPU_VAR: &str = "E2E_DISABLE_GPU";

/// `builder` with the webview's remote debugging port on a free port of 127.0.0.1, which the
/// browser picks and writes to `DevToolsActivePort`.
pub(crate) fn with_debugging_port<R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'_, R, M>,
) -> WebviewWindowBuilder<'_, R, M> {
    let mut args = format!("{WRY_DEFAULT_ARGS} --remote-debugging-port=0");
    if std::env::var(DISABLE_GPU_VAR).is_ok_and(|value| value == "1") {
        args.push_str(" --disable-gpu");
    }
    builder.additional_browser_args(&args)
}
