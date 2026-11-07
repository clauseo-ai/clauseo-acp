//! Which top-level navigations the app's windows take: only to the app's own pages.
//!
//! Contract:
//! - Tauri 2.12.4 lets a webview go anywhere unless a handler says no (`plugin.rs`, `on_navigation`
//!   defaults to `true`), and the CSP has no `navigate-to`. Page script could then send data out
//!   with `location.href = "https://host/?data"` or `<meta http-equiv=refresh>`. [`guard`] gives
//!   each window (`main` and `settings`) a navigation handler that asks [`allowed`], and refuses
//!   the rest with one warning that names the scheme and host only, never the path or query.
//! - [`allowed`] takes:
//!   - the origin Tauri serves the bundled pages from, [`APP_SCHEME`]`://`[`APP_HOST`]: both
//!     windows set `use_https_scheme(true)`, so `https://tauri.localhost` on Windows and
//!     `tauri://localhost` elsewhere (tauri `protocol/mod.rs`, `origin`). Any path, query or hash
//!     within it: the settings window's `settings.html#section`, `index.html`. That origin serves
//!     the embedded assets and nothing else, with no redirects (tauri `manager/mod.rs`,
//!     `get_asset`), and a redirect is a navigation the handler sees again;
//!   - `about:blank` and `about:srcdoc`, which load nothing: `WKWebView` and `WebKitGTK` ask the
//!     handler for frames too (wry `wkwebview/navigation.rs` and `webkitgtk/mod.rs` do not check
//!     the frame), and an empty frame starts at one of them;
//!   - in a development build only, the `devUrl` origin. [`dev_url`] passes it on only when
//!     `tauri::is_dev()`, which is `!cfg!(feature = "custom-protocol")`, the switch Tauri itself
//!     uses to load `devUrl` (`manager/mod.rs`, `get_app_url`). `tauri build`, which makes every
//!     release, e2e and marks build, turns on `custom-protocol`, so those builds refuse
//!     `localhost:<port>` whatever the config says.
//! - Same origin means the same scheme, host and port, and no user name or password in the URL.
//!   `clauseo-file:` is refused: the page uses it as an `img` source only, never a page.
//! - On Windows `WebView2` asks only for the top frame (`NavigationStarting`); frames' navigations
//!   are held by the CSP's `frame-src 'self'` there.
//! - A URL the `url` crate cannot parse is let through by tauri-runtime-wry before this sees it;
//!   a webview never navigates to one.
//! - `window.open` stays refused by wry, since no window sets `on_new_window`.

use tauri::{Manager, Runtime, Url, WebviewWindowBuilder};

#[cfg(test)]
mod tests;

/// The scheme of the origin Tauri serves the app's pages from, with `use_https_scheme(true)`.
#[cfg(any(windows, target_os = "android"))]
const APP_SCHEME: &str = "https";
#[cfg(not(any(windows, target_os = "android")))]
const APP_SCHEME: &str = "tauri";

/// The host of that origin.
#[cfg(any(windows, target_os = "android"))]
const APP_HOST: &str = "tauri.localhost";
#[cfg(not(any(windows, target_os = "android")))]
const APP_HOST: &str = "localhost";

/// `builder` with a navigation handler that refuses whatever [`allowed`] refuses.
pub(crate) fn guard<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    app: &M,
) -> WebviewWindowBuilder<'a, R, M> {
    let dev = dev_url(app.config().build.dev_url.as_ref()).cloned();
    builder.on_navigation(move |url| {
        let allow = allowed(url, dev.as_ref());
        if !allow {
            tracing::warn!(
                scheme = url.scheme(),
                host = url.host_str().unwrap_or(""),
                "navigation refused: not the app's own page"
            );
        }
        allow
    })
}

/// `devUrl` in a development build; `None` in any build made with `custom-protocol`.
pub(crate) fn dev_url(configured: Option<&Url>) -> Option<&Url> {
    configured.filter(|_| tauri::is_dev())
}

/// Whether a window may navigate to `url`: the app's origin, an empty page, or `dev`'s origin.
pub(crate) fn allowed(url: &Url, dev: Option<&Url>) -> bool {
    if !url.username().is_empty() || url.password().is_some() {
        return false;
    }
    if url.scheme() == "about" {
        return matches!(url.path(), "blank" | "srcdoc");
    }
    let app =
        url.scheme() == APP_SCHEME && url.host_str() == Some(APP_HOST) && url.port().is_none();
    app || dev.is_some_and(|dev| {
        url.scheme() == dev.scheme()
            && dev.host_str().is_some()
            && url.host_str() == dev.host_str()
            && url.port_or_known_default() == dev.port_or_known_default()
    })
}
