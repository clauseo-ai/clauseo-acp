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

/// Which button of a confirm is the default, the one Return answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DefaultButton {
    /// Cancel, so a stray key never accepts: every confirm but the link's.
    Cancel,
    /// The accepting button: the link's confirm only, where the lawyer has just clicked the link.
    Ok,
}

/// The buttons of a confirm with `default` first. rfd draws the first button as the default on
/// both OSes: macOS's alert (`NSAlert`, or `CFUserNotification` with no window) makes it the
/// Return button, and Windows' task dialog, given no default, focuses it (Return, Enter and
/// Space). Escape answers rfd's plain `Cancel` on Windows and the button titled "Cancel" on
/// macOS, whichever is first. See [`cancel_first`] and [`ok_first`].
pub(crate) fn buttons(default: DefaultButton, ok: &str, cancel: &str) -> MessageButtons {
    match default {
        DefaultButton::Cancel => cancel_first(ok, cancel),
        DefaultButton::Ok => ok_first(ok, cancel),
    }
}

/// The buttons of a confirm: `cancel` first, so it is the default and focused button (Return
/// and Enter on both OSes, Space on Windows) and a stray key never accepts. `ok` second.
pub(crate) fn cancel_first(ok: &str, cancel: &str) -> MessageButtons {
    MessageButtons::OkCancelCustom(cancel.to_owned(), ok.to_owned())
}

/// The buttons of the link's confirm: `ok` first, so it is the default and focused button
/// (Return and Enter on both OSes, Space on Windows). `cancel` second.
pub(crate) fn ok_first(ok: &str, cancel: &str) -> MessageButtons {
    MessageButtons::OkCancelCustom(ok.to_owned(), cancel.to_owned())
}

/// Whether a confirm drawn with [`buttons`], in either order, was accepted: only rfd's answer for
/// a press of the `ok` button, `Custom(ok)`. Its plain `Cancel` (Escape, Alt+F4 or the close
/// button on Windows, a stopped sheet on macOS, a dialog that failed), `Custom(cancel)` and
/// anything else decline.
pub(crate) fn accepted(result: &MessageDialogResult, ok: &str) -> bool {
    matches!(result, MessageDialogResult::Custom(label) if label == ok)
}

/// Draws a native confirm on `app`, over `owner`'s window (see [`owner_label`]; main when that
/// window is gone), with [`buttons`] in `default`'s order, and answers [`accepted`]. rfd is
/// called directly, as the dialog plugin itself calls it (built on the main thread, awaited on a
/// thread of its own), because the plugin turns rfd's plain `Cancel` into
/// `Custom(<second label>)`, which with Cancel first is the accepting label. A dialog that never
/// answers (the app closing, no main thread, no thread) counts as declined.
pub(crate) fn confirm<R: Runtime>(
    app: &AppHandle<R>,
    owner: Owner,
    default: DefaultButton,
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
    let buttons = buttons(default, &ok, cancel);
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

/// Where the picker opens, as the page asks (`start`). Absent it is `Last`, as `"last"` is.
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum StartHint {
    /// This run's last pick's parent, else (Windows) Documents when the share check passed, else
    /// home (macOS: no folder, the OS's own remembered one).
    Last,
    /// The user's Documents folder.
    Documents,
    /// Where new matters go: `matters.default_location`, or the default Documents folder.
    Matters,
    /// A folder, by its absolute path: `{"path": "..."}`. Ignored unless absolute and non-empty.
    #[serde(rename = "path")]
    Folder(String),
}

/// What the picker's start folder is chosen from. Each is read before the choice, without the
/// disk: no stat, no exists, no canonicalize.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StartFolders<'a> {
    /// This run's last pick, as the dialog gave it.
    pub(crate) last_pick: Option<&'a Path>,
    /// The user's home folder.
    pub(crate) home: Option<&'a Path>,
    /// The user's Documents folder.
    pub(crate) documents: Option<&'a Path>,
    /// Where new matters go (`matters.default_location`, or the default folder).
    pub(crate) matters: Option<&'a Path>,
    /// Windows' known folders, as the share check reads them; empty elsewhere.
    pub(crate) known: &'a [PathBuf],
    /// The share check's cached answer: `Some(true)` when Windows' dialog would stall,
    /// `Some(false)` when it would not, `None` when it has not run or not finished.
    pub(crate) stalls: Option<bool>,
}

/// Where the picker opens for `hint` (`None` is `Last`). On Windows (`on_windows`):
/// - a hint is honoured unless it names a folder inside a known folder while the share check has
///   not passed (`stalls` is not `Some(false)`), since Windows' dialog would stall there; then
///   the default below is used;
/// - the default is the last pick's parent, else Documents when the check passed, else home.
///
/// Elsewhere a hint is honoured, and the default is the last pick's parent, else no folder: the
/// OS's own remembered folder. A folder that no longer exists is passed through as it is.
pub(crate) fn start_for(
    hint: Option<&StartHint>,
    on_windows: bool,
    folders: &StartFolders<'_>,
) -> Option<PathBuf> {
    let hinted = hint.and_then(|hint| hinted(hint, folders)).filter(|dir| {
        !on_windows || folders.stalls == Some(false) || !inside_known(dir, folders.known)
    });
    if let Some(dir) = hinted {
        return Some(dir);
    }
    let beside = folders
        .last_pick
        .and_then(Path::parent)
        .filter(|dir| usable(dir))
        .map(Path::to_path_buf);
    if !on_windows {
        return beside;
    }
    let passed = folders.stalls == Some(false);
    beside
        .or_else(|| {
            folders
                .documents
                .filter(|_| passed)
                .filter(|dir| usable(dir))
                .map(Path::to_path_buf)
        })
        .or_else(|| {
            folders
                .home
                .filter(|dir| usable(dir))
                .map(Path::to_path_buf)
        })
}

/// The folder `hint` names, when it names a usable one (`None` for `Last`).
fn hinted(hint: &StartHint, folders: &StartFolders<'_>) -> Option<PathBuf> {
    let dir = match hint {
        StartHint::Last => return None,
        StartHint::Documents => folders.documents,
        StartHint::Matters => folders.matters,
        StartHint::Folder(path) => Some(Path::new(path)),
    }?;
    usable(dir).then(|| dir.to_path_buf())
}

/// The folder where `matters.default_location` (or the default) is, from the host's
/// `GetDefaultLocation` `display`: as it is when absolute, else below `home`. `None` for an empty
/// display, or a relative one with no home.
pub(crate) fn matters_from_display(display: &str, home: Option<&Path>) -> Option<PathBuf> {
    if display.is_empty() {
        return None;
    }
    let path = Path::new(display);
    if path.is_absolute() {
        Some(path.to_path_buf())
    } else {
        home.map(|home| home.join(path))
    }
}

/// Whether `dir` is one of `known` or below it, compared as Windows does: case blind, with `\`
/// and `/` alike. Lexical only.
fn inside_known(dir: &Path, known: &[PathBuf]) -> bool {
    let dir = comparable(dir);
    known.iter().any(|folder| {
        let folder = comparable(folder);
        !folder.is_empty()
            && (dir == folder
                || dir
                    .strip_prefix(folder.as_str())
                    .is_some_and(|rest| rest.starts_with('\\')))
    })
}

/// `path` as the comparison reads it: `\` separators, lower case, no trailing separator.
fn comparable(path: &Path) -> String {
    path.to_string_lossy()
        .replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
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

/// The native folder dialog for "Add a skill", over the Settings window when it is open. It
/// picks folders only: the dialog plugin draws a folder dialog or a file dialog, never one for
/// both, so a lone `SKILL.md` is added by picking the folder that holds it. It keeps no last
/// pick, so the matter picker's start folder stays the matters'.
pub(crate) struct SkillPicker<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> SkillPicker<R> {
    /// A picker drawing on `app`.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> Picker for SkillPicker<R> {
    fn pick(&self) -> BoxFuture<'_, Option<PathBuf>> {
        let (tx, rx) = oneshot::channel();
        let mut dialog = self.app.dialog().file();
        let label = owner_label(
            Owner::SettingsOrMain,
            self.app.get_webview_window(SETTINGS_WINDOW).is_some(),
        );
        if let Some(parent) = self.app.get_webview_window(label) {
            dialog = dialog.set_parent(&parent);
        }
        dialog.pick_folder(move |picked| {
            // The command may have gone; then nobody reads the answer.
            let _gone = tx.send(picked.and_then(|path| path.into_path().ok()));
        });
        Box::pin(async move { rx.await.ok().flatten() })
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
                DefaultButton::Cancel,
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
