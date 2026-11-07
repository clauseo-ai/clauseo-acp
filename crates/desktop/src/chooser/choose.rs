//! `chooser_choose`: the folder the chooser shows becomes a pick only after a native confirm
//! drawn by Rust (`docs/threat-model.md`: escalations only through native dialogs, never a
//! webview confirm), then goes through the bridge's own `pick_folder` with a one-shot picker, so
//! the folder is registered and issued exactly as the native dialog's pick is.

use std::path::{Path, PathBuf};

use clauseo_protocol::HostError;
use rfd::MessageLevel;
use tauri::{AppHandle, Runtime};

use super::Chooser;
use super::listing::{LIST_WAIT, bounded};
use crate::bridge::Bridge;
use crate::dialogs::{BoxFuture, LastPick, Picker};
use crate::picked::PickedFolder;

/// The confirm's words; the message is the folder's full path.
const TITLE: &str = "Use this folder for the matter?";
const OK: &str = "Use this folder";
const CANCEL: &str = "Cancel";

/// Asks the lawyer to confirm the folder `path`: `true` to use it.
pub(crate) trait FolderConfirm: Send + Sync {
    /// Asks once.
    fn confirm(&self, path: &str) -> BoxFuture<'_, bool>;
}

/// The native confirm, drawn by Rust with Cancel first, so focused (`dialogs::confirm`).
pub(crate) struct NativeFolderConfirm<R: Runtime> {
    app: AppHandle<R>,
}

impl<R: Runtime> NativeFolderConfirm<R> {
    /// A confirm drawing on `app`.
    pub(crate) fn new(app: AppHandle<R>) -> Self {
        Self { app }
    }
}

impl<R: Runtime> FolderConfirm for NativeFolderConfirm<R> {
    fn confirm(&self, path: &str) -> BoxFuture<'_, bool> {
        crate::dialogs::confirm(
            &self.app,
            MessageLevel::Info,
            TITLE.to_owned(),
            format!("{TITLE}\n\n{path}"),
            OK.to_owned(),
            CANCEL,
        )
    }
}

/// The picker that gives the one folder the lawyer confirmed.
struct Chosen(PathBuf);

impl Picker for Chosen {
    fn pick(&self) -> BoxFuture<'_, Option<PathBuf>> {
        let folder = self.0.clone();
        Box::pin(async move { Some(folder) })
    }
}

/// Chooses the folder session `chooser` shows: checks it is still a folder and no link (on a
/// thread, at most `LIST_WAIT`), asks `confirm`, then registers it through
/// `Bridge::pick_folder`, records it in `last` and closes the session. `None` when the check
/// fails or the lawyer declines; the session stays open then. `command.invalid` for an unknown,
/// closed or expired session, or one that shows no folder yet.
pub(crate) async fn choose_folder(
    chooser: &Chooser,
    bridge: &Bridge,
    confirm: &dyn FolderConfirm,
    last: &LastPick,
    home: Option<&Path>,
    id: u32,
) -> Result<Option<PickedFolder>, HostError> {
    let path = chooser.current(id, crate::open::now())?;
    let machine = std::sync::Arc::clone(&chooser.machine);
    let outstanding = std::sync::Arc::clone(&chooser.outstanding);
    let checked = path.clone();
    let check = move || {
        bounded(&outstanding, LIST_WAIT, move || {
            machine
                .entry(&checked)
                .is_some_and(|entry| entry.dir && !entry.link)
        })
    };
    let plain = tauri::async_runtime::spawn_blocking(check).await;
    if !matches!(plain, Ok(Some(true))) {
        return Ok(None);
    }
    if !confirm.confirm(&path).await {
        return Ok(None);
    }
    let folder = PathBuf::from(&path);
    let picked = bridge.pick_folder(&Chosen(folder.clone()), home).await?;
    if picked.is_some() {
        last.set(folder);
    }
    // Gone already when another session replaced it meanwhile; the pick stands.
    drop(chooser.close(id));
    Ok(picked)
}
