//! `chooser_choose`: the folder the chooser shows becomes a pick only after a native confirm
//! drawn by Rust (`docs/threat-model.md`: escalations only through native dialogs, never a
//! webview confirm), then goes through the bridge's own `pick_folder` with a one-shot picker, so
//! the folder is registered and issued exactly as the native dialog's pick is.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use clauseo_protocol::HostError;
use rfd::MessageLevel;
use tauri::{AppHandle, Runtime};

use super::Chooser;
use super::listing::{LIST_WAIT, bounded};
use crate::bridge::Bridge;
use crate::dialogs::{BoxFuture, LastPick, Picker};
use crate::picked::PickedFolder;

#[cfg(test)]
mod tests;

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
            format!("{TITLE}\n\n{}", escaped(path)),
            OK.to_owned(),
            CANCEL,
        )
    }
}

/// The format characters (Unicode `Cf`) [`escaped`] shows as code points, as inclusive ranges:
/// the bidi controls (U+061C, U+200E/F, U+202A-202E, U+2066-2069) among them. Not ZWNJ and ZWJ
/// (U+200C/D): Devanagari and other Indic scripts write with them (`क्‍ष`), and they reorder
/// nothing.
const FORMAT: [(u32, u32); 21] = [
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x0890, 0x0891),
    (0x08E2, 0x08E2),
    (0x180E, 0x180E),
    (0x200B, 0x200B),
    (0x200E, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xFFF9, 0xFFFB),
    (0x110BD, 0x110BD),
    (0x110CD, 0x110CD),
    (0x13430, 0x1343F),
    (0x1BCA0, 0x1BCA3),
    (0x1D173, 0x1D17A),
    (0xE0001, 0xE007F),
];

/// `path` as the confirm shows it: each control character (C0, DEL, C1) and format character
/// of [`FORMAT`] as `<U+XXXX>`, so a folder name cannot reorder or hide where the path points.
/// Letters of every script stay as they are. Display only: the folder registered is `path`.
pub(crate) fn escaped(path: &str) -> String {
    let mut shown = String::with_capacity(path.len());
    for c in path.chars() {
        let point = u32::from(c);
        let format = FORMAT
            .iter()
            .any(|&(low, high)| (low..=high).contains(&point));
        if c.is_control() || format {
            // Writing to a String does not fail.
            let _infallible = write!(shown, "<U+{point:04X}>");
        } else {
            shown.push(c);
        }
    }
    shown
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
