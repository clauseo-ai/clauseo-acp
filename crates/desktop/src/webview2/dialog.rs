//! What the dialog does with what Windows answered: the decisions of `windows.rs`, without
//! Windows.

use super::words::Words;

/// The action button's id (W6), which opens Microsoft's page. Only a set with an action has it.
pub(super) const DOWNLOAD: i32 = 100;

/// The "Close" button's id. Esc and the title bar's close button answer `IDCANCEL` instead;
/// both only close.
pub(super) const CLOSE: i32 = 101;

/// The task dialog's buttons, in order, as (id, label): the action (W6) when the words have one,
/// then Close. The first is the default button.
pub(super) fn buttons(words: &Words) -> Vec<(i32, &'static str)> {
    words
        .action
        .map(|action| (DOWNLOAD, action))
        .into_iter()
        .chain([(CLOSE, words.close)])
        .collect()
}

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

/// The plain message box's two buttons, as (OK's label, Cancel's label), when the words have an
/// action: the box shows OK and Cancel renamed to the action (W6) and Close (W7), so the
/// fallback offers the download too (#858). `None`: OK alone, as the words have only Close.
pub(super) fn message_box_labels(words: &Words) -> Option<(&'static str, &'static str)> {
    words.action.map(|action| (action, words.close))
}

/// `MessageBoxW`'s answer for OK (`IDOK`), which carries the action's label when there is one.
pub(super) const MESSAGE_BOX_OK: i32 = 1;

/// What to do after the plain message box answered `answer`: open the download page only for
/// OK on a box with the action (`message_box_labels`); anything else (Close, Esc, the title
/// bar's close button, a failed box) only closes.
pub(super) fn after_message_box(words: &Words, answer: i32) -> Next {
    if words.action.is_some() && answer == MESSAGE_BOX_OK {
        Next::Download
    } else {
        Next::Close
    }
}

/// The message box's text: the main line, then the body, as the task dialog says them.
pub(super) fn message_box_text(words: &Words) -> String {
    format!("{}\n\n{}", words.instruction, words.content)
}
