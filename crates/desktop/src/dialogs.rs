//! The native dialogs the commands draw in Rust: the folder picker (the dialog plugin) and the
//! confirms, "always allow" and the link's (rfd, called directly: see [`confirm`]). Each sits
//! behind a trait, so tests answer without a window, and the `e2e` build answers the picker and
//! "always allow" from its variables (`e2e.rs`). The webview has no permission for the dialog
//! plugin.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use futures::channel::oneshot;
use rfd::{MessageButtons, MessageDialogResult, MessageLevel};
use tauri::{AppHandle, Manager as _, Runtime};
use tauri_plugin_dialog::DialogExt as _;

/// A boxed future, so the traits stay object safe.
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Picks a folder: `None` when the lawyer cancelled.
pub(crate) trait Picker: Send + Sync {
    /// Asks for one folder.
    fn pick(&self) -> BoxFuture<'_, Option<PathBuf>>;
}

/// Asks the lawyer to confirm an "always allow" answer: `true` to go ahead.
pub(crate) trait Confirm: Send + Sync {
    /// Asks once.
    fn confirm(&self) -> BoxFuture<'_, bool>;
}

/// The confirm's words. Fixed text: nothing the agent wrote reaches the dialog.
const CONFIRM_TITLE: &str = "Always allow?";
const CONFIRM_MESSAGE: &str = "The agent will do actions like this one without asking you again.";
const CONFIRM_OK: &str = "Always allow";
const CONFIRM_CANCEL: &str = "Cancel";

/// The buttons of a confirm: `cancel` first, so it is the default and focused button (Return
/// and Enter on both OSes, Space on Windows) and a stray key never accepts. `ok` second.
pub(crate) fn cancel_first(ok: &str, cancel: &str) -> MessageButtons {
    MessageButtons::OkCancelCustom(cancel.to_owned(), ok.to_owned())
}

/// Whether a confirm drawn with [`cancel_first`] was accepted: only rfd's answer for a press of
/// the `ok` button, `Custom(ok)`. Its plain `Cancel` (Escape, Alt+F4 or the close button on
/// Windows, a stopped sheet on macOS, a dialog that failed) and anything else decline.
pub(crate) fn accepted(result: &MessageDialogResult, ok: &str) -> bool {
    matches!(result, MessageDialogResult::Custom(label) if label == ok)
}

/// Draws a native confirm on `app`, over the main window, with [`cancel_first`]'s buttons, and
/// answers [`accepted`]. rfd is called directly, as the dialog plugin itself calls it (built on
/// the main thread, awaited on a thread of its own), because the plugin turns rfd's plain
/// `Cancel` into `Custom(<second label>)`, which with Cancel first is the accepting label. A
/// dialog that never answers (the app closing, no main thread, no thread) counts as declined.
pub(crate) fn confirm<R: Runtime>(
    app: &AppHandle<R>,
    level: MessageLevel,
    title: String,
    message: String,
    ok: String,
    cancel: &str,
) -> BoxFuture<'static, bool> {
    let (tx, rx) = oneshot::channel();
    let main = app.get_webview_window(crate::MAIN_WINDOW);
    let buttons = cancel_first(&ok, cancel);
    // Not posted (the event loop has gone): the closure and its sender drop, which declines.
    let _unposted = app.run_on_main_thread(move || {
        let mut dialog = rfd::AsyncMessageDialog::new()
            .set_title(&title)
            .set_description(&message)
            .set_level(level)
            .set_buttons(buttons);
        if let Some(main) = &main {
            dialog = dialog.set_parent(main);
        }
        let shown = dialog.show();
        // No thread: the sender drops with the closure, which declines.
        let _no_thread = std::thread::Builder::new()
            .name("clauseo-confirm".to_owned())
            .spawn(move || {
                let result = futures::executor::block_on(shown);
                // The command may have gone; then nobody reads the answer.
                let _gone = tx.send(accepted(&result, &ok));
            });
    });
    Box::pin(async move { rx.await.unwrap_or(false) })
}

/// The native folder dialog, drawn by Rust.
pub(crate) struct NativePicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> NativePicker<R> {
    /// A picker drawing on `app`.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> Picker for NativePicker<R> {
    fn pick(&self) -> BoxFuture<'_, Option<PathBuf>> {
        #[cfg(feature = "e2e")]
        if let Some(folder) = crate::e2e::matter_dir() {
            return Box::pin(async move { Some(folder) });
        }
        let (tx, rx) = oneshot::channel();
        self.app.dialog().file().pick_folder(move |picked| {
            // The command may have gone; then nobody reads the answer.
            let _gone = tx.send(picked.and_then(|path| path.into_path().ok()));
        });
        Box::pin(async move { rx.await.ok().flatten() })
    }
}

/// The native confirm dialog, drawn by Rust.
pub(crate) struct NativeConfirm<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> NativeConfirm<R> {
    /// A confirm drawing on `app`.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> Confirm for NativeConfirm<R> {
    fn confirm(&self) -> BoxFuture<'_, bool> {
        #[cfg(feature = "e2e")]
        if let Some(answer) = crate::e2e::confirm_always() {
            return Box::pin(async move { answer });
        }
        confirm(
            &self.app,
            MessageLevel::Warning,
            CONFIRM_TITLE.to_owned(),
            CONFIRM_MESSAGE.to_owned(),
            CONFIRM_OK.to_owned(),
            CONFIRM_CANCEL,
        )
    }
}

#[cfg(test)]
mod tests;
