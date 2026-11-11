//! The lawyer's MCP servers (Settings › Tools) on the desktop side:
//!
//! - their keychain: the connectors' keychain (`connectors::secret_store`), under its own
//!   service, `clauseo_settings::MCP_KEYCHAIN_SERVICE` ([`with_mcp_secrets`]);
//! - the native confirm before a save adds a server or changes what starts
//!   ([`with_mcp_confirm`]), drawn by Rust as the pay link's is (`connectors.rs`,
//!   `open/native.rs`), so a script in the settings window can't make the host run a program
//!   the lawyer never saw. Its words are in [`words`].
//!
//! Kept out of `connectors.rs`, which reads no settings. Nothing here logs the program, its
//! arguments or the URL.

mod words;

use std::sync::Arc;

use clauseo_connectors::KeyringStore;
use clauseo_host::{HostConfig, McpLaunchAsk, McpLaunchConfirm};
use clauseo_protocol::Language;
use tauri::{AppHandle, Manager as _, Runtime};

use crate::bridge::Bridge;
use crate::dialogs::BoxFuture;

use words::LaunchWords;

/// `config` with that keychain; unchanged with none (`None`, as on Linux), so no value can be
/// saved and no server naming one starts.
pub(crate) fn with_mcp_secrets(config: HostConfig, store: Option<KeyringStore>) -> HostConfig {
    match store {
        Some(store) => config.with_mcp_secrets(Arc::new(
            store.with_service(clauseo_settings::MCP_KEYCHAIN_SERVICE),
        )),
        None => config,
    }
}

/// `config` with the native confirm on `app` ([`LaunchConfirm`] over [`NativeLaunchDrawer`]).
pub(crate) fn with_mcp_confirm<R: Runtime>(config: HostConfig, app: AppHandle<R>) -> HostConfig {
    config.with_mcp_confirm(Arc::new(LaunchConfirm(NativeLaunchDrawer { app })))
}

/// Draws the confirm, so tests answer without a window.
pub(crate) trait LaunchDrawer: Send + Sync {
    /// The app's language (`ui.language`), which the words follow.
    fn language(&self) -> BoxFuture<'_, Language>;
    /// Draws `words`: `true` for Allow; Cancel, Escape, closing, and a dialog that never
    /// answers are `false`.
    fn draw(&self, words: LaunchWords) -> BoxFuture<'_, bool>;
}

/// The host's [`McpLaunchConfirm`]: the words for the ask, in the app's language, drawn by `D`.
pub(crate) struct LaunchConfirm<D>(pub(crate) D);

impl<D: LaunchDrawer> McpLaunchConfirm for LaunchConfirm<D> {
    /// Blocks the host's keychain thread (never the actor, never a runtime thread) until the
    /// lawyer answers, as `LinkOpener::open_confirmed` does.
    fn confirm(&self, ask: &McpLaunchAsk) -> bool {
        let drawer = &self.0;
        tauri::async_runtime::block_on(async {
            let language = drawer.language().await;
            drawer.draw(LaunchWords::new(ask, language)).await
        })
    }
}

/// The confirm drawn by Rust with `dialogs::confirm`: Cancel first and default, over the main
/// window. The webview has no permission for the dialog plugin.
pub(crate) struct NativeLaunchDrawer<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> LaunchDrawer for NativeLaunchDrawer<R> {
    fn language(&self) -> BoxFuture<'_, Language> {
        Box::pin(async move {
            match self.app.try_state::<Bridge>() {
                Some(bridge) => bridge.language().await,
                // Not up yet: the settings window it is asked from can't be either; English.
                None => Language::En,
            }
        })
    }

    fn draw(&self, words: LaunchWords) -> BoxFuture<'_, bool> {
        crate::dialogs::confirm(
            &self.app,
            rfd::MessageLevel::Warning,
            words.title,
            words.message,
            words.ok,
            &words.cancel,
        )
    }
}

#[cfg(test)]
mod tests;
