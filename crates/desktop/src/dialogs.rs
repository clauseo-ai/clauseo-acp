//! The two native dialogs the commands draw in Rust: the folder picker and the "always allow"
//! confirm. Each sits behind a trait, so tests answer without a window, and the `e2e` build
//! answers from its variables (`e2e.rs`). The webview has no permission for the dialog plugin.

use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use futures::channel::oneshot;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt as _, MessageDialogButtons, MessageDialogKind};

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
        let (tx, rx) = oneshot::channel();
        self.app
            .dialog()
            .message(CONFIRM_MESSAGE)
            .title(CONFIRM_TITLE)
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::OkCancelCustom(
                CONFIRM_OK.to_owned(),
                CONFIRM_CANCEL.to_owned(),
            ))
            .show(move |ok| {
                let _gone = tx.send(ok);
            });
        // A dialog that never answers (the app closing) counts as declined.
        Box::pin(async move { rx.await.unwrap_or(false) })
    }
}
