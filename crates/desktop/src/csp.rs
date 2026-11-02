//! The page's Content-Security-Policy on the app's scripts too.
//!
//! Contract:
//! - Tauri 2.12.4 sends the `Content-Security-Policy` header with `.html` assets only
//!   (`manager/mod.rs`, `get_asset`: `csp_header` is set under `is_html`), and
//!   `app.security.headers` has no field for it. A dedicated worker takes its policy from its own
//!   script's response, so the code-colouring worker (`ui/components`, `highlight/worker.ts`)
//!   ran with none.
//! - [`on_scripts`] gives each window (`main` and `settings`) a web-resource hook that adds
//!   `app.security.csp` from the checked config, as written, to every JavaScript response of
//!   the app's protocol that has no policy. The page keeps Tauri's own header, which adds the
//!   nonces and hashes of its inline code; a script has no inline code to hash. The policy
//!   string lives in `tauri.conf.json` only.
//! - Tauri keeps only the last `on_web_resource_request` handler of a builder, so this file is the
//!   only place in the crate that sets one (a test holds the crate to it).
//! - Development builds load `devUrl` from Vite's server, which this hook never sees.

use std::borrow::Cow;

use tauri::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE};
use tauri::http::{HeaderValue, Response};
use tauri::{Manager, Runtime, WebviewWindowBuilder};

/// The content type Tauri gives `.js` and `.mjs` assets (tauri-utils `MimeType::Js`). Not
/// `application/javascript`: Tauri never types an asset so, and a browser refuses a worker script
/// whose type is not a JavaScript one, so every script the app can start arrives as this.
const JAVASCRIPT: &str = "text/javascript";

/// `app.security.csp` from the app's config, as a header value. `None` when the config has no
/// CSP, or when its string is not a valid header value ([`header_value`]); either way no hook is
/// installed, and the page goes without a CSP too, since Tauri sends the same string.
pub(crate) fn policy<R: Runtime, M: Manager<R>>(app: &M) -> Option<HeaderValue> {
    let csp = app.config().app.security.csp.as_ref()?;
    header_value(&csp.to_string())
}

/// `csp` as a header value; `None` when it holds a byte a header cannot (a line break, say).
fn header_value(csp: &str) -> Option<HeaderValue> {
    HeaderValue::from_str(csp).ok()
}

/// Whether `content_type` is JavaScript: its media type, before any `;` parameter, trimmed and
/// compared without regard to case.
fn is_javascript(content_type: &HeaderValue) -> bool {
    content_type
        .to_str()
        .ok()
        .and_then(|value| value.split(';').next())
        .is_some_and(|media| media.trim().eq_ignore_ascii_case(JAVASCRIPT))
}

/// Adds `policy` to `response` when it is JavaScript and carries no policy yet. Assets a browser
/// could open as a page under another type get none: an unknown extension, which tauri-utils
/// types `text/html` without the page's CSP, or `.svg`. `ui/app/dist` has neither today; if it
/// ever does, put the policy on every response that has none.
pub(crate) fn add_to_script(policy: &HeaderValue, response: &mut Response<Cow<'static, [u8]>>) {
    let headers = response.headers_mut();
    let script = headers.get(CONTENT_TYPE).is_some_and(is_javascript);
    if script && !headers.contains_key(CONTENT_SECURITY_POLICY) {
        headers.insert(CONTENT_SECURITY_POLICY, policy.clone());
    }
}

/// `builder` with the hook that gives the app's scripts the config's policy.
pub(crate) fn on_scripts<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    app: &M,
) -> WebviewWindowBuilder<'a, R, M> {
    match policy(app) {
        Some(policy) => builder.on_web_resource_request(move |_request, response| {
            add_to_script(&policy, response);
        }),
        None => builder,
    }
}

#[cfg(test)]
mod tests;
