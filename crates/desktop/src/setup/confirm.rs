//! The native confirm in front of a window's `SetFirmEndpoint` (decision record 0048): Rust
//! draws it, so a webview cannot point an agent at another server without the lawyer's say.
//! `InstallAgent` asks nothing: it is the lawyer choosing the agent's card, which shows the
//! download's size first. A decline runs nothing and answers
//! `transport.unauthorized`, as `answer_permission`'s does. The words are fixed text and the
//! values the backend fills in (the agent's name and the endpoint's address,
//! checked before the confirm as the settings crate checks it, `backend.rs`'s
//! `checked_endpoint`); Cancel is the default button.
//!
//! A confirm has no timeout: it waits until the lawyer answers. Cancel, Escape, closing the
//! dialog, a stopped sheet when the main window closes, or a dialog that never shows (the app
//! closing) each decline (`dialogs::accepted`, `dialogs::confirm`), so the command runs
//! nothing. A command dropped while its confirm is up (its window gone) runs nothing either:
//! the answer reaches no one, and nothing waits on it.

use rfd::MessageLevel;
use tauri::{AppHandle, Runtime};

use crate::dialogs::{BoxFuture, confirm};

/// What a setup confirm asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ask {
    /// Send the agent named `name` to the firm's endpoint `url`, or with `None` stop.
    Endpoint {
        /// The agent's name.
        name: String,
        /// The endpoint, or `None` to clear it.
        url: Option<String>,
    },
}

/// Asks the lawyer a setup confirm: `true` to go ahead.
pub(crate) trait SetupConfirm: Send + Sync {
    /// Asks `ask` once.
    fn confirm(&self, ask: Ask) -> BoxFuture<'_, bool>;
}

/// The words of `ask`: title, message and the accepting button.
pub(crate) fn words(ask: &Ask) -> (String, String, String) {
    match ask {
        Ask::Endpoint {
            name,
            url: Some(url),
        } => (
            format!("Use your firm's endpoint for {name}?"),
            format!("{name} will send your work to {url} instead of its usual service."),
            "Use this endpoint".to_owned(),
        ),
        Ask::Endpoint { name, url: None } => (
            format!("Stop using your firm's endpoint for {name}?"),
            format!("{name} will go back to its usual service."),
            "Stop using it".to_owned(),
        ),
    }
}

/// The native setup confirm, drawn by Rust over the main window.
pub(crate) struct NativeSetupConfirm<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> NativeSetupConfirm<R> {
    /// A confirm drawing on `app`.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> SetupConfirm for NativeSetupConfirm<R> {
    fn confirm(&self, ask: Ask) -> BoxFuture<'_, bool> {
        #[cfg(feature = "e2e")]
        if let Some(answer) = crate::e2e::confirm_always() {
            return Box::pin(async move { answer });
        }
        let (title, message, ok) = words(&ask);
        confirm(&self.app, MessageLevel::Info, title, message, ok, "Cancel")
    }
}
