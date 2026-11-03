//! The `WebView2` check on Windows: before any window, the app makes sure the runtime that draws
//! its windows is there, and new enough.
//!
//! Contract:
//! - [`stops_start`] runs first in `run`, before Tauri builds anything. On Windows it reads the
//!   installed runtime's version (`tauri::webview_version`, which is wry's call to
//!   `GetAvailableCoreWebView2BrowserVersionString`). When the runtime is missing, or older than
//!   [`version::MINIMUM`] (`bundle.windows.minimumWebview2Version` in `tauri.conf.json`; a test
//!   holds the two together), it shows a task dialog and returns `true`, and `run` returns
//!   without building the app: never a blank window. Elsewhere it does nothing.
//! - The dialog (`windows.rs`) says the words of `words.rs`, in English only for now. Its two
//!   buttons: the download button (W6) opens Microsoft's bootstrapper page ([`DOWNLOAD_URL`])
//!   in the default browser, then the app exits; "Close" (W7) exits.
//! - A version the runtime reports in a form we do not read lets the app start: Tauri tries the
//!   runtime as it is, rather than this check refusing one that may work.
//! - The installer fetches the runtime itself (`webviewInstallMode`, `downloadBootstrapper`), so
//!   the dialog is for a runtime removed or held back after installing, or a copy of the exe run
//!   without its installer. `docs/decisions/0035-release-builds.md` records the choices.

#[cfg(any(windows, test))]
mod version;
#[cfg(windows)]
mod windows;
#[cfg(any(windows, test))]
mod words;

#[cfg(test)]
mod tests;

/// Microsoft's page for the Evergreen bootstrapper: what the download button opens.
#[cfg(any(windows, test))]
const DOWNLOAD_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// Whether the app must not start: on Windows, the runtime is missing or too old, and the dialog
/// that says so has been shown and closed. Always `false` elsewhere.
pub(crate) fn stops_start() -> bool {
    #[cfg(windows)]
    return windows::check();
    #[cfg(not(windows))]
    return false;
}
