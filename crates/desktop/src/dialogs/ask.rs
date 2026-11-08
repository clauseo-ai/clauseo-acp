//! R18's native dialogs: the host's `NativeDialogs`, drawn over the main window in the app's
//! language (`ui_language::current`), with words from [`super::words`].
//!
//! - Windows: a task dialog (`windows.rs`) with Allow and Don't allow, and a "Remember for this
//!   matter" checkbox; Don't allow is the default button.
//! - macOS and Linux (`rfd_ask.rs`): rfd's message dialog with three buttons, Allow once,
//!   Always allow for this matter and Don't allow; there is no "Don't allow for this matter".
//!   macOS waits for `NSAlert` with its suppression checkbox, which needs `objc2-app-kit`, a
//!   dependency the architecture table does not list yet (a decision record first).
//! - The pickers: the dialog plugin's file dialog, a folder or a file per `Pick::files_too`.
//! - The `e2e` build answers from `E2E_DIALOG_ANSWERS` and `E2E_DIALOG_PICKS` when either is
//!   set (`script.rs`).
//!
//! Contract: Escape, the close button and a dialog that failed or never showed refuse
//! ([`answer`]); a cancelled or failed picker gives `None`. Every call blocks; the host makes
//! them off its actor, and a call on the main thread, which would wait on itself, refuses at
//! once. No path reaches the log.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_host::{Dialog, DialogAnswer, HostConfig, NativeDialogs, Pick};
use tauri::{AppHandle, Manager as _, Runtime};
use tauri_plugin_dialog::DialogExt as _;

mod answer;
#[cfg(not(windows))]
mod rfd_ask;
#[cfg(any(feature = "e2e", test))]
mod script;
#[cfg(windows)]
mod windows;
mod words;

#[cfg(test)]
mod tests;

/// `config` with the app's native dialogs: in the `e2e` build the scripted ones when their
/// variables are set, else those drawn on `app`.
pub(crate) fn with_dialogs<R: Runtime>(config: HostConfig, app: &AppHandle<R>) -> HostConfig {
    #[cfg(feature = "e2e")]
    if let Some(scripted) = script::Scripted::from_env() {
        return config.with_dialogs(Arc::new(scripted));
    }
    config.with_dialogs(Arc::new(DesktopDialogs { app: app.clone() }))
}

/// The native dialogs, drawn on the app's main window.
struct DesktopDialogs<R: Runtime> {
    app: AppHandle<R>,
}

/// Whether this is the main thread, where a blocking dialog call would wait on itself.
fn on_main_thread() -> bool {
    std::thread::current().name() == Some("main")
}

impl<R: Runtime> NativeDialogs for DesktopDialogs<R> {
    fn ask(&self, dialog: Dialog) -> DialogAnswer {
        if on_main_thread() {
            tracing::warn!("a widening dialog was asked on the main thread: refused");
            return DialogAnswer::Refuse;
        }
        let words = words::AskWords::new(crate::ui_language::current(), &dialog);
        let main = self.app.get_webview_window(crate::MAIN_WINDOW);
        #[cfg(windows)]
        let answer = windows::ask(main.as_ref(), &words);
        #[cfg(not(windows))]
        let answer = rfd_ask::ask(&self.app, main, &words);
        answer
    }

    fn pick(&self, pick: Pick) -> Option<PathBuf> {
        if on_main_thread() {
            tracing::warn!("a picker was asked on the main thread: cancelled");
            return None;
        }
        let title = words::pick_title(crate::ui_language::current(), &pick);
        let mut dialog = self
            .app
            .dialog()
            .file()
            .set_title(title)
            .set_directory(&pick.start);
        if let Some(main) = self.app.get_webview_window(crate::MAIN_WINDOW) {
            dialog = dialog.set_parent(&main);
        }
        let (tx, rx) = std::sync::mpsc::channel();
        let answer = move |picked: Option<tauri_plugin_dialog::FilePath>| {
            // The caller may have gone; then nobody reads the pick.
            let _gone = tx.send(picked.and_then(|path| path.into_path().ok()));
        };
        // The plugin draws on the main thread; a dialog that never answers drops the sender.
        if pick.files_too {
            dialog.pick_file(answer);
        } else {
            dialog.pick_folder(answer);
        }
        rx.recv().ok().flatten()
    }
}
