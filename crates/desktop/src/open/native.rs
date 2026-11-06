//! The native sides of [`super::LinkConfirm`] and [`super::Opener`]: the confirm drawn by
//! Rust with the dialog plugin (`dialogs::confirm`: Cancel first and default, over the main
//! window), and the OS's default apps and file manager through the opener plugin. The webview
//! has no permission for either plugin.

use std::path::{Path, PathBuf};

use clauseo_host::HostHandle;
use clauseo_protocol::{ErrorCode, FileRefId, HostError, MatterId, Principal};
use tauri::{AppHandle, Runtime};
use tauri_plugin_opener::OpenerExt as _;

use super::{LinkConfirm, LinkWords, Locate, LocatedFile, Opener};
use crate::dialogs::BoxFuture;

/// The link confirm, drawn by Rust.
pub(crate) struct NativeLinkConfirm<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> NativeLinkConfirm<R> {
    /// A confirm drawing on `app`.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> LinkConfirm for NativeLinkConfirm<R> {
    fn confirm_link(&self, words: LinkWords) -> BoxFuture<'_, bool> {
        crate::dialogs::confirm(
            &self.app,
            rfd::MessageLevel::Info,
            words.title,
            words.message,
            words.ok,
            &words.cancel,
        )
    }
}

/// The OS's default apps and file manager.
pub(crate) struct NativeOpener<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> NativeOpener<R> {
    /// An opener on `app`.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

/// The app to open files of `kind` with, by path: on macOS `LaunchServices`' default for the
/// type (`mac_app`), so a file swapped for a link to a program after the checks opens as a
/// document in that app; `internal` when macOS knows none. Elsewhere `None`: Windows picks the
/// app by the name's extension, which was checked, and Linux is not a release platform.
#[cfg(target_os = "macos")]
fn default_app(kind: &str) -> Result<Option<String>, HostError> {
    let app = super::mac_app::default_app(kind).ok_or_else(|| {
        HostError::new(
            ErrorCode::Internal,
            format!("no app opens files of the type {kind}"),
        )
    })?;
    let app = app.to_str().map(str::to_owned).ok_or_else(|| {
        HostError::new(ErrorCode::Internal, "the default app's path is not Unicode")
    })?;
    Ok(Some(app))
}

#[cfg(not(target_os = "macos"))]
fn default_app(_kind: &str) -> Option<String> {
    None
}

/// An `internal` failure: the OS did not open what it was asked to.
fn not_opened(what: &str, error: &tauri_plugin_opener::Error) -> HostError {
    HostError::new(
        ErrorCode::Internal,
        format!("the system did not open the {what}: {error}"),
    )
}

impl<R: Runtime> Opener for NativeOpener<R> {
    fn open_url(&self, url: &str) -> Result<(), HostError> {
        self.app
            .opener()
            .open_url(url, None::<&str>)
            .map_err(|error| not_opened("link", &error))
    }

    fn open_path(&self, path: &Path, kind: &str) -> Result<(), HostError> {
        // A lossy name could name another file: one that is not Unicode is refused.
        let path = path.to_str().ok_or_else(|| {
            HostError::new(ErrorCode::CommandInvalid, "the file's name is not Unicode")
        })?;
        #[cfg(target_os = "macos")]
        let app = default_app(kind)?;
        #[cfg(not(target_os = "macos"))]
        let app = default_app(kind);
        self.app
            .opener()
            .open_path(path, app.as_deref())
            .map_err(|error| not_opened("file", &error))
    }

    fn reveal(&self, path: &Path) -> Result<(), HostError> {
        self.app
            .opener()
            .reveal_item_in_dir(path)
            .map_err(|error| not_opened("folder", &error))
    }

    #[cfg(windows)]
    fn short_folder(&self, folder: &Path) -> Option<PathBuf> {
        super::word_path::short_folder(folder)
    }
}

/// The host answers both as the local desktop, the principal of every bridge call.
impl Locate for HostHandle {
    fn locate_file(&self, file: FileRefId) -> BoxFuture<'_, Result<LocatedFile, HostError>> {
        Box::pin(HostHandle::locate_file(self, Principal::LocalDesktop, file))
    }

    fn matter_folder(&self, matter: MatterId) -> BoxFuture<'_, Result<PathBuf, HostError>> {
        Box::pin(HostHandle::matter_folder(
            self,
            Principal::LocalDesktop,
            matter,
        ))
    }
}
