//! The native confirm in front of a window's `SetFirmEndpoint` (decision record 0048): Rust
//! draws it, so a webview cannot point an agent at another server without the lawyer's say.
//! `InstallAgent` asks nothing: it is the lawyer choosing the agent's card, which shows the
//! download's size first. A decline runs nothing and answers
//! `transport.unauthorized`, as `answer_permission`'s does. One more stands in front of a Codex
//! sign-in while Codex is signed in with Clauseo's own login (`Ask::SwitchAccount`, since
//! `codex login` signs that account out first); its decline runs nothing and answers `Done`.
//! The words are fixed text (`dialogs/words.rs`, in the app's language as
//! `ui_language::current` last saw it) and the values the backend fills in (the agent's name
//! and the endpoint's address, checked before the confirm as the settings crate checks it,
//! `backend.rs`'s `checked_endpoint`); Cancel is the default button.
//!
//! A confirm has no timeout: it waits until the lawyer answers. Cancel, Escape, closing the
//! dialog, a stopped sheet when the main window closes, or a dialog that never shows (the app
//! closing) each decline (`dialogs::accepted`, `dialogs::confirm`), so the command runs
//! nothing. A command dropped while its confirm is up (its window gone) runs nothing either:
//! the answer reaches no one, and nothing waits on it.

use clauseo_protocol::Language;
use rfd::MessageLevel;
use tauri::{AppHandle, Runtime};

use crate::dialogs::words::{ConfirmWords, Key};
use crate::dialogs::{BoxFuture, Owner, confirm};

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
    /// Sign the agent named `name` (Codex) out of its account so another can sign in.
    SwitchAccount {
        /// The agent's name.
        name: String,
    },
}

/// Asks the lawyer a setup confirm: `true` to go ahead.
pub(crate) trait SetupConfirm: Send + Sync {
    /// Asks `ask` once.
    fn confirm(&self, ask: Ask) -> BoxFuture<'_, bool>;
}

/// The words of `ask` in `language`.
pub(crate) fn words(ask: &Ask, language: Language) -> ConfirmWords {
    match ask {
        Ask::Endpoint {
            name,
            url: Some(url),
        } => ConfirmWords::new(
            language,
            [Key::EndpointTitle, Key::EndpointMessage, Key::EndpointOk],
            &[("name", name), ("url", url)],
        ),
        Ask::Endpoint { name, url: None } => ConfirmWords::new(
            language,
            [Key::ClearTitle, Key::ClearMessage, Key::ClearOk],
            &[("name", name)],
        ),
        Ask::SwitchAccount { name } => ConfirmWords::new(
            language,
            [Key::SwitchTitle, Key::SwitchMessage, Key::SwitchOk],
            &[("name", name)],
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
        let words = words(&ask, crate::ui_language::current());
        let (title, message, ok) = (words.title, words.message, words.ok);
        confirm(
            &self.app,
            Owner::Main,
            MessageLevel::Info,
            title,
            message,
            ok,
            &words.cancel,
        )
    }
}
