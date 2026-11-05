//! The native confirms in front of a window's `InstallAgent` and `SetFirmEndpoint` (decision
//! record 0048): Rust draws them, so a webview cannot install an agent or point one at another
//! server without the lawyer's say. A decline runs nothing and answers
//! `transport.unauthorized`, as `answer_permission`'s does. The words are fixed text and the
//! values the backend fills in (the agent's name, the download size, the endpoint's address,
//! already checked by the settings crate once accepted); Cancel is the default button.

use rfd::MessageLevel;
use tauri::{AppHandle, Runtime};

use crate::dialogs::{BoxFuture, confirm};

/// What a setup confirm asks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Ask {
    /// Install the agent named `name`, downloading about `megabytes` MB.
    Install {
        /// The agent's name, as the snapshot gives it.
        name: String,
        /// The download, in MB (10^6 bytes), rounded up; `None` when unknown.
        megabytes: Option<u64>,
    },
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
        Ask::Install { name, megabytes } => {
            let size = megabytes.map_or_else(String::new, |mb| format!(" (about {mb} MB)"));
            (
                format!("Install {name}?"),
                format!("Clauseo will download {name}{size} and install it for you."),
                "Install".to_owned(),
            )
        }
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
