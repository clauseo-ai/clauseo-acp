//! The lawyer's MCP servers (Settings › Tools) on the desktop side:
//!
//! - the finder of Settings › Tools' import ([`with_mcp_import`]);
//! - their keychain: the connectors' keychain (`connectors::secret_store`), under its own
//!   service, `clauseo_settings::MCP_KEYCHAIN_SERVICE` ([`with_mcp_secrets`]);
//! - the native confirm before a save adds a server or changes what starts
//!   ([`with_mcp_confirm`]), drawn by Rust as the pay link's is (`connectors.rs`,
//!   `open/native.rs`), so a script in the settings window can't make the host run a program
//!   the lawyer never saw. Its words are in [`words`].
//!
//! Kept out of `connectors.rs`, which reads no settings. Nothing here logs the program, its
//! arguments or the URL.

mod import_words;
mod words;

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use clauseo_connectors::KeyringStore;
use clauseo_host::{HostConfig, McpLaunchAsk, McpLaunchConfirm};
use clauseo_protocol::Language;
use tauri::{AppHandle, Manager as _, Runtime};

use crate::bridge::{Bridge, lock};
use crate::dialogs::{BoxFuture, Owner};

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
    config.with_mcp_confirm(Arc::new(LaunchConfirm::new(NativeLaunchDrawer { app })))
}

/// Draws the confirm, so tests answer without a window.
pub(crate) trait LaunchDrawer: Send + Sync {
    /// The app's language (`ui.language`), which the words follow.
    fn language(&self) -> BoxFuture<'_, Language>;
    /// Draws `words`: `true` for Allow; Cancel, Escape, closing, and a dialog that never
    /// answers are `false`.
    fn draw(&self, words: LaunchWords) -> BoxFuture<'_, bool>;
}

/// How long after a Cancel the same ask is declined with no dialog.
pub(crate) const SAME_ASK_QUIET: Duration = Duration::from_secs(10);

/// The clock the flood control reads: `open::gate::now` in the app, a hand-moved one in tests.
pub(crate) type Clock = Arc<dyn Fn() -> Instant + Send + Sync>;

/// The host's [`McpLaunchConfirm`]: the words for the ask, in the app's language, drawn by `D`.
/// Flood control, so a script in the settings window can't wear the lawyer down: at most one
/// dialog at a time (another ask meanwhile is declined at once), and after a Cancel the same ask
/// within [`SAME_ASK_QUIET`] is declined with no dialog.
pub(crate) struct LaunchConfirm<D> {
    drawer: D,
    now: Clock,
    flood: Mutex<Flood>,
}

/// What the flood control remembers.
#[derive(Default)]
struct Flood {
    /// Whether a dialog is open.
    open: bool,
    /// The last ask declined (one save's, or an import's list), and when.
    declined: Option<(Vec<McpLaunchAsk>, Instant)>,
}

impl<D> LaunchConfirm<D> {
    /// A confirm drawn by `drawer`, on the app's clock.
    pub(crate) fn new(drawer: D) -> Self {
        Self::with_clock(drawer, Arc::new(crate::open::now))
    }

    /// A confirm drawn by `drawer`, on `now`.
    pub(crate) fn with_clock(drawer: D, now: Clock) -> Self {
        Self {
            drawer,
            now,
            flood: Mutex::new(Flood::default()),
        }
    }

    /// The drawer (tests).
    #[cfg(test)]
    pub(crate) fn drawer(&self) -> &D {
        &self.drawer
    }
}

impl<D: LaunchDrawer> LaunchConfirm<D> {
    /// Draws the words `words` makes in the app's language, for `asks`, behind the flood
    /// control: declined at once while another dialog is open, or when `asks` is the ask just
    /// cancelled within [`SAME_ASK_QUIET`]. Blocks the host's keychain thread (never the actor,
    /// never a runtime thread) until the lawyer answers, as `LinkOpener::open_confirmed` does.
    fn gated(&self, asks: &[McpLaunchAsk], words: impl FnOnce(Language) -> LaunchWords) -> bool {
        {
            let at = (self.now)();
            let mut flood = lock(&self.flood);
            let just_declined = flood.declined.as_ref().is_some_and(|(declined, when)| {
                declined.as_slice() == asks && at < *when + SAME_ASK_QUIET
            });
            if flood.open || just_declined {
                return false;
            }
            flood.open = true;
        }
        let drawer = &self.drawer;
        let allowed = tauri::async_runtime::block_on(async {
            let language = drawer.language().await;
            drawer.draw(words(language)).await
        });
        let mut flood = lock(&self.flood);
        flood.open = false;
        flood.declined = (!allowed).then(|| (asks.to_vec(), (self.now)()));
        allowed
    }
}

impl<D: LaunchDrawer> McpLaunchConfirm for LaunchConfirm<D> {
    /// One save's confirm ([`Self::gated`]).
    fn confirm(&self, ask: &McpLaunchAsk) -> bool {
        self.gated(std::slice::from_ref(ask), |language| {
            LaunchWords::new(ask, language)
        })
    }

    /// One confirm for the whole import, listing each server ([`import_words`]), behind the
    /// same flood control.
    fn confirm_import(&self, asks: &[McpLaunchAsk]) -> bool {
        self.gated(asks, |language| import_words::import_words(asks, language))
    }
}

/// The confirm drawn by Rust with `dialogs::confirm`: Cancel first and default, over the Settings
/// window when it is open (the lawyer is asking from there), else the main window. The webview
/// has no permission for the dialog plugin.
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
            Owner::SettingsOrMain,
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

/// `config` finding the MCP servers Claude Desktop, Claude Code and Codex set up on this
/// computer, for Settings › Tools' import.
pub(crate) fn with_mcp_import(config: HostConfig) -> HostConfig {
    config.with_mcp_import(Arc::new(clauseo_host::McpHereFinder))
}
