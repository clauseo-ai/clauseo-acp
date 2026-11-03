//! Windows: reads the runtime's version and, when it is missing or too old, shows the task
//! dialog. Nothing here runs once Tauri has started.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 task dialog, DPI awareness, COM and ShellExecuteW"
)]

use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TDF_ALLOW_DIALOG_CANCELLATION, TaskDialogIndirect,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
use windows::core::{HSTRING, PCWSTR, w};

use super::DOWNLOAD_URL;
use super::version::{MINIMUM, Verdict, Version, verdict};
use super::words::{EN, Words};

/// The "Download WebView2" button's id.
const DOWNLOAD: i32 = 100;

/// The "Close" button's id. Esc and the title bar's close button answer `IDCANCEL` instead;
/// both only close.
const CLOSE: i32 = 101;

/// Reads the runtime's version and, when it is missing or too old, shows the dialog and acts on
/// its answer. `true` when the app must not start.
pub(super) fn check() -> bool {
    let installed = tauri::webview_version().ok();
    // MINIMUM is a literal a test parses, so this never returns.
    let Some(minimum) = Version::parse(MINIMUM) else {
        return false;
    };
    match verdict(installed.as_deref(), minimum) {
        Verdict::Start => false,
        Verdict::Missing | Verdict::TooOld => {
            if show(&EN) == Some(DOWNLOAD) {
                open_download_page();
            }
            true
        }
    }
}

/// Shows the dialog and returns the id of the button that closed it, `None` when it could not
/// be shown.
fn show(words: &Words) -> Option<i32> {
    // Sharp text on a scaled display. The process exits after the dialog, before Tauri would
    // set its own awareness, so nothing else depends on it. Best effort: blurry text still reads.
    // SAFETY: a plain call with a predefined context value.
    drop(unsafe { SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) });
    let title = HSTRING::from(words.title);
    let instruction = HSTRING::from(words.instruction);
    let content = HSTRING::from(words.content);
    let footer = HSTRING::from(words.footer);
    let download = HSTRING::from(words.download);
    let close = HSTRING::from(words.close);
    let buttons = [
        TASKDIALOG_BUTTON {
            nButtonID: DOWNLOAD,
            pszButtonText: PCWSTR(download.as_ptr()),
        },
        TASKDIALOG_BUTTON {
            nButtonID: CLOSE,
            pszButtonText: PCWSTR(close.as_ptr()),
        },
    ];
    let config = TASKDIALOGCONFIG {
        cbSize: u32::try_from(size_of::<TASKDIALOGCONFIG>()).ok()?,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        pszFooter: PCWSTR(footer.as_ptr()),
        cButtons: u32::try_from(buttons.len()).ok()?,
        pButtons: buttons.as_ptr(),
        nDefaultButton: DOWNLOAD,
        ..TASKDIALOGCONFIG::default()
    };
    let mut pressed = 0_i32;
    // SAFETY: `config` and every string and button it points to live until the call returns;
    // `pressed` is a valid out pointer. comctl32 6 (the app manifest) has TaskDialogIndirect.
    unsafe { TaskDialogIndirect(&raw const config, Some(&raw mut pressed), None, None) }.ok()?;
    Some(pressed)
}

/// Opens [`DOWNLOAD_URL`] in the default browser. Best effort: if it fails, the dialog's words
/// still name what to install.
fn open_download_page() {
    // ShellExecuteW may hand the URL to a shell extension through COM (its documentation asks
    // for an apartment first). The process exits next, so nothing uninitialises it.
    // SAFETY: a plain call; the reserved pointer is null as required.
    drop(unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED | COINIT_DISABLE_OLE1DDE) }.ok());
    let url = HSTRING::from(DOWNLOAD_URL);
    // A value of 32 or less is an error; there is nothing left to tell the user.
    // SAFETY: every string is a valid null-terminated wide string that outlives the call; no
    // owner window.
    let _ = unsafe {
        ShellExecuteW(
            None,
            w!("open"),
            PCWSTR(url.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        )
    };
}
