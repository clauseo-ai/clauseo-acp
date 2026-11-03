//! What the dialog does with what Windows answered: the decisions of `windows.rs`, without
//! Windows.

use super::words::Words;

/// The download button's id (W6).
pub(super) const DOWNLOAD: i32 = 100;

/// The "Close" button's id. Esc and the title bar's close button answer `IDCANCEL` instead;
/// both only close.
pub(super) const CLOSE: i32 = 101;

/// What to do after the task dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Next {
    /// Open Microsoft's download page, then exit.
    Download,
    /// Exit.
    Close,
    /// The task dialog failed (no icon resource it could load, no interactive desktop, ...):
    /// show the plain message box, then exit.
    MessageBox,
}

/// From the task dialog's outcome: `Ok` with the id of the button that closed it, or `Err` when
/// `TaskDialogIndirect` returned a failure.
pub(super) fn next<E>(pressed: &Result<i32, E>) -> Next {
    match pressed {
        Ok(DOWNLOAD) => Next::Download,
        Ok(_) => Next::Close,
        Err(_) => Next::MessageBox,
    }
}

/// The exe's icon resource, which tauri-build embeds under this id.
pub(super) const APP_ICON: u16 = tauri::utils::platform::WINDOWS_APP_ICON_RESOURCE_ID;

/// The icon resource the dialog names: [`APP_ICON`] only when the exe holds it, since
/// `TaskDialogIndirect` may fail outright on a resource id it cannot load (a test binary, a
/// build without tauri-build's icon). `None`: no icon, and no module handle.
pub(super) fn main_icon(resource_found: bool) -> Option<u16> {
    resource_found.then_some(APP_ICON)
}

/// The message box's text: the main line, then the body, as the task dialog says them.
pub(super) fn message_box_text(words: &Words) -> String {
    format!("{}\n\n{}", words.instruction, words.content)
}
