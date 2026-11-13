//! The native dialogs the commands draw in Rust: the folder picker (the dialog plugin) and the
//! confirms, "always allow" and the link's (rfd, called directly: see [`confirm`]). Each sits
//! behind a trait, so tests answer without a window, and the `e2e` build answers the picker and
//! "always allow" from its variables (`e2e.rs`). The webview has no permission for the dialog
//! plugin. The confirms speak the app's language: the link's from `open/words.rs`, the others
//! from [`words`].

use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Mutex, PoisonError};

use clauseo_protocol::Language;
use futures::channel::oneshot;
use rfd::{MessageButtons, MessageDialogResult, MessageLevel};
use tauri::{AppHandle, Manager as _, Runtime};
use tauri_plugin_dialog::DialogExt as _;

use crate::MAIN_WINDOW;
use crate::settings_window::SETTINGS_WINDOW;

/// A boxed future, so the traits stay object safe.
pub(crate) type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// The window a confirm sits over, so the OS brings it to the front where the lawyer is asking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Owner {
    /// The main window, always.
    Main,
    /// The Settings window when it is open (a Settings › Tools confirm, asked from there), else
    /// the main window.
    SettingsOrMain,
}

/// The label of the window a confirm sits over: [`Owner::SettingsOrMain`] takes Settings only
/// when `settings_open`; everything else takes main.
pub(crate) fn owner_label(owner: Owner, settings_open: bool) -> &'static str {
    match owner {
        Owner::SettingsOrMain if settings_open => SETTINGS_WINDOW,
        Owner::Main | Owner::SettingsOrMain => MAIN_WINDOW,
    }
}

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

pub(crate) mod words;

use words::{ConfirmWords, Key};

/// "Always allow"'s words in `language`. Fixed text: nothing the agent wrote reaches the dialog.
pub(crate) fn always_allow_words(language: Language) -> ConfirmWords {
    let keys = [Key::AlwaysTitle, Key::AlwaysMessage, Key::AlwaysOk];
    ConfirmWords::new(language, keys, &[])
}

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

/// Draws a native confirm on `app`, over `owner`'s window (see [`owner_label`]; main when that
/// window is gone), with [`cancel_first`]'s buttons, and answers [`accepted`]. rfd is called
/// directly, as the dialog plugin itself calls it (built on the main thread, awaited on a thread
/// of its own), because the plugin turns rfd's plain `Cancel` into `Custom(<second label>)`,
/// which with Cancel first is the accepting label. A dialog that never answers (the app closing,
/// no main thread, no thread) counts as declined.
pub(crate) fn confirm<R: Runtime>(
    app: &AppHandle<R>,
    owner: Owner,
    level: MessageLevel,
    title: String,
    message: String,
    ok: String,
    cancel: &str,
) -> BoxFuture<'static, bool> {
    let (tx, rx) = oneshot::channel();
    let label = owner_label(owner, app.get_webview_window(SETTINGS_WINDOW).is_some());
    let parent = app
        .get_webview_window(label)
        .or_else(|| app.get_webview_window(MAIN_WINDOW));
    let buttons = cancel_first(&ok, cancel);
    // Not posted (the event loop has gone): the closure and its sender drop, which declines.
    let _unposted = app.run_on_main_thread(move || {
        let mut dialog = rfd::AsyncMessageDialog::new()
            .set_title(&title)
            .set_description(&message)
            .set_level(level)
            .set_buttons(buttons);
        if let Some(parent) = &parent {
            dialog = dialog.set_parent(parent);
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

/// The folder a picker opens on: the parent of the last pick, else `home`, each only when it is a
/// non-empty absolute path. Nothing here touches the disk.
pub(crate) fn start_folder(last_pick: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    let beside = last_pick.and_then(Path::parent).filter(|dir| usable(dir));
    beside
        .or_else(|| home.filter(|dir| usable(dir)))
        .map(Path::to_path_buf)
}

/// Whether `dir` can be a starting folder: a non-empty absolute path.
fn usable(dir: &Path) -> bool {
    !dir.as_os_str().is_empty() && dir.is_absolute()
}

/// The folder the last pick in this session chose. In memory only: a relaunch starts over.
#[derive(Default)]
pub(crate) struct LastPick(Mutex<Option<PathBuf>>);

impl LastPick {
    /// The folder the last pick chose, if any.
    pub(crate) fn get(&self) -> Option<PathBuf> {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Records `folder` as the last pick (the native dialog's, or the chooser's).
    pub(crate) fn set(&self, folder: PathBuf) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(folder);
    }
}

/// The native folder dialog, drawn by Rust.
pub(crate) struct NativePicker<'a, R: Runtime> {
    app: AppHandle<R>,
    start: Option<PathBuf>,
    last: &'a LastPick,
}

impl<'a, R: Runtime> NativePicker<'a, R> {
    /// A picker drawing on `app`, opening on `start` when there is one; a folder picked is
    /// recorded in `last`.
    pub(crate) fn new(app: AppHandle<R>, start: Option<PathBuf>, last: &'a LastPick) -> Self {
        Self { app, start, last }
    }
}

impl<R: Runtime> Picker for NativePicker<'_, R> {
    fn pick(&self) -> BoxFuture<'_, Option<PathBuf>> {
        #[cfg(feature = "e2e")]
        if let Some(folder) = crate::e2e::matter_dir() {
            return Box::pin(async move { Some(folder) });
        }
        let (tx, rx) = oneshot::channel();
        let mut dialog = self.app.dialog().file();
        if let Some(start) = &self.start {
            dialog = dialog.set_directory(start);
        }
        dialog.pick_folder(move |picked| {
            // The command may have gone; then nobody reads the answer.
            let _gone = tx.send(picked.and_then(|path| path.into_path().ok()));
        });
        let last = self.last;
        Box::pin(async move {
            let picked = rx.await.ok().flatten();
            if let Some(folder) = &picked {
                last.set(folder.clone());
            }
            picked
        })
    }
}

/// The native "always allow" confirm, drawn by Rust in the app's language.
pub(crate) struct NativeConfirm<'a, R: Runtime> {
    app: AppHandle<R>,
    bridge: &'a crate::bridge::Bridge,
}

impl<'a, R: Runtime> NativeConfirm<'a, R> {
    /// A confirm drawing on `app`, in the language `bridge` reads when it is asked.
    pub(crate) fn new(app: AppHandle<R>, bridge: &'a crate::bridge::Bridge) -> Self {
        Self { app, bridge }
    }
}

impl<R: Runtime> Confirm for NativeConfirm<'_, R> {
    fn confirm(&self) -> BoxFuture<'_, bool> {
        #[cfg(feature = "e2e")]
        if let Some(answer) = crate::e2e::confirm_always() {
            return Box::pin(async move { answer });
        }
        Box::pin(async move {
            let words = always_allow_words(self.bridge.language().await);
            let level = MessageLevel::Warning;
            let (title, message, ok) = (words.title, words.message, words.ok);
            confirm(
                &self.app,
                Owner::Main,
                level,
                title,
                message,
                ok,
                &words.cancel,
            )
            .await
        })
    }
}

#[cfg(test)]
mod tests;
