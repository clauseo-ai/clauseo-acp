//! The native sides of [`super::LinkConfirm`] and [`super::Opener`]: the confirm drawn by
//! Rust with the dialog plugin, and the OS's default apps and file manager through the opener
//! plugin. The webview has no permission for either plugin.

use std::path::{Path, PathBuf};

use clauseo_host::HostHandle;
use clauseo_protocol::{ErrorCode, FileRefId, HostError, MatterId};
use futures::channel::oneshot;
use tauri::{AppHandle, Runtime};
use tauri_plugin_dialog::{DialogExt as _, MessageDialogButtons, MessageDialogKind};
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
        let (tx, rx) = oneshot::channel();
        self.app
            .dialog()
            .message(words.message)
            .title(words.title)
            .kind(MessageDialogKind::Info)
            .buttons(MessageDialogButtons::OkCancelCustom(words.ok, words.cancel))
            .show(move |ok| {
                // The command may have gone; then nobody reads the answer.
                let _gone = tx.send(ok);
            });
        // A dialog that never answers (the app closing) counts as declined.
        Box::pin(async move { rx.await.unwrap_or(false) })
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

    fn open_path(&self, path: &Path) -> Result<(), HostError> {
        // A lossy name could name another file: one that is not Unicode is refused.
        let path = path.to_str().ok_or_else(|| {
            HostError::new(ErrorCode::CommandInvalid, "the file's name is not Unicode")
        })?;
        self.app
            .opener()
            .open_path(path, None::<&str>)
            .map_err(|error| not_opened("file", &error))
    }

    fn reveal(&self, path: &Path) -> Result<(), HostError> {
        self.app
            .opener()
            .reveal_item_in_dir(path)
            .map_err(|error| not_opened("folder", &error))
    }
}

/// PLACEHOLDER until the host's `locate_file` and `matter_folder` land (they follow #131):
/// both refuse with `internal`.
impl Locate for HostHandle {
    fn locate_file(&self, file: FileRefId) -> BoxFuture<'_, Result<LocatedFile, HostError>> {
        Box::pin(async move {
            Err(HostError::new(
                ErrorCode::Internal,
                format!("the host cannot locate file {file} yet"),
            ))
        })
    }

    fn matter_folder(&self, matter: MatterId) -> BoxFuture<'_, Result<PathBuf, HostError>> {
        Box::pin(async move {
            Err(HostError::new(
                ErrorCode::Internal,
                format!("the host cannot find matter {matter}'s folder yet"),
            ))
        })
    }
}
