//! The `e2e` feature, Windows only: WebView2 opens a DevTools port on 127.0.0.1 so Playwright can
//! attach with `connectOverCDP`, and the port is printed to stdout as `E2E_CDP_PORT=<port>`.
//!
//! Contract:
//! - Built only with `--features e2e` on Windows. The default build has no `e2e` feature, so a
//!   release never opens a debugging port.
//! - `CLAUSEO_E2E_DISABLE_GPU=1` adds `--disable-gpu`, for the second CI run.
//! - The port line is printed before the window is built; the browser may open the port a moment
//!   later, so the harness retries its connection.

use std::io::Write as _;
use std::net::{Ipv4Addr, TcpListener};

use tauri::{Manager, Runtime, WebviewWindowBuilder};

/// wry's own browser arguments. Passing `additional_browser_args` replaces them (wry
/// `src/webview2/mod.rs`), so they are passed again here.
const WRY_DEFAULT_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// Set to `1` to run the webview without the GPU.
const DISABLE_GPU_VAR: &str = "CLAUSEO_E2E_DISABLE_GPU";

/// `builder` with WebView2's DevTools on a free port of 127.0.0.1, after printing the port.
pub(crate) fn with_debugging_port<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
) -> tauri::Result<WebviewWindowBuilder<'a, R, M>> {
    // The OS picks a port nothing listens on; the listener closes before WebView2 binds it.
    let port = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?
        .local_addr()?
        .port();
    let mut args = format!("{WRY_DEFAULT_ARGS} --remote-debugging-port={port}");
    if std::env::var(DISABLE_GPU_VAR).is_ok_and(|value| value == "1") {
        args.push_str(" --disable-gpu");
    }
    writeln!(std::io::stdout().lock(), "E2E_CDP_PORT={port}")?;
    Ok(builder.additional_browser_args(&args))
}
