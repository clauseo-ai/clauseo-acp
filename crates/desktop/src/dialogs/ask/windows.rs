//! Windows: a widening question as a task dialog with Allow and Don't allow, Don't allow the
//! default, and the "Remember for this matter" checkbox (the verification checkbox). Shown on
//! the calling thread, owned by the main window, as rfd shows its own dialogs.
#![expect(unsafe_code, reason = "FFI to the Win32 task dialog")]

use clauseo_host::DialogAnswer;
use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TDF_ALLOW_DIALOG_CANCELLATION,
    TDF_POSITION_RELATIVE_TO_WINDOW, TaskDialogIndirect,
};
use windows::core::{BOOL, HSTRING, PCWSTR};

use super::answer::{ALLOW, REFUSE, from_task_dialog};
use super::words::AskWords;

/// Asks `words`'s question over `main`, when there is one, and waits for the answer. A dialog
/// that fails refuses.
pub(super) fn ask<R: Runtime>(main: Option<&WebviewWindow<R>>, words: &AskWords) -> DialogAnswer {
    let parent = main.and_then(|main| main.hwnd().ok());
    match task_dialog(parent, words) {
        Ok((pressed, remember)) => from_task_dialog(pressed, remember),
        Err(err) => {
            tracing::warn!(code = err.code().0, "the widening dialog failed: refused");
            DialogAnswer::Refuse
        }
    }
}

/// Shows the dialog: the button pressed (`IDCANCEL` for Escape, Alt+F4 and the close button)
/// and whether the checkbox was checked.
fn task_dialog(parent: Option<HWND>, words: &AskWords) -> windows::core::Result<(i32, bool)> {
    let title = HSTRING::from("Clauseo");
    let instruction = HSTRING::from(words.title.as_str());
    let content = HSTRING::from(words.message.as_str());
    let remember = HSTRING::from(words.remember.as_str());
    let allow = HSTRING::from(words.allow.as_str());
    let refuse = HSTRING::from(words.refuse.as_str());
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: ALLOW,
            pszButtonText: PCWSTR(allow.as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: REFUSE,
            pszButtonText: PCWSTR(refuse.as_ptr()),
        },
    ];
    let config = TASKDIALOGCONFIG {
        // Both always fit in a u32.
        cbSize: u32::try_from(size_of::<TASKDIALOGCONFIG>()).unwrap_or(u32::MAX),
        hwndParent: parent.unwrap_or_default(),
        // Escape, Alt+F4 and the close button answer IDCANCEL; centred on the main window.
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION | TDF_POSITION_RELATIVE_TO_WINDOW,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        cButtons: u32::try_from(buttons.len()).unwrap_or(u32::MAX),
        pButtons: buttons.as_ptr(),
        // Return and Space never allow by accident.
        nDefaultButton: REFUSE,
        pszVerificationText: PCWSTR(remember.as_ptr()),
        ..TASKDIALOGCONFIG::default()
    };
    let mut pressed = 0_i32;
    let mut checked = BOOL::default();
    // SAFETY: `config` and every string and button it points to live until the call returns;
    // `pressed` and `checked` are valid out pointers. comctl32 6 (the app manifest) has
    // TaskDialogIndirect; the parent, when set, is the main window's live handle.
    unsafe {
        TaskDialogIndirect(
            &raw const config,
            Some(&raw mut pressed),
            None,
            Some(&raw mut checked),
        )
    }?;
    Ok((pressed, checked.as_bool()))
}
