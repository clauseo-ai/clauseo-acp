//! Windows: reads the runtime's version and, when it is missing or too old, shows the task
//! dialog. Nothing here runs once Tauri has started.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 task dialog, DPI awareness, COM and ShellExecuteW"
)]

use windows::Win32::Foundation::{HINSTANCE, HMODULE};
use windows::Win32::System::Com::{
    COINIT_APARTMENTTHREADED, COINIT_DISABLE_OLE1DDE, CoInitializeEx,
};
use windows::Win32::System::LibraryLoader::{FindResourceW, GetModuleHandleW};
use windows::Win32::UI::Controls::{
    TASKDIALOG_BUTTON, TASKDIALOGCONFIG, TASKDIALOGCONFIG_0, TDF_ALLOW_DIALOG_CANCELLATION,
    TaskDialogIndirect,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::WindowsAndMessaging::{
    MB_ICONWARNING, MB_OK, MessageBoxW, RT_GROUP_ICON, SW_SHOWNORMAL,
};
use windows::core::{HSTRING, PCWSTR, w};

use super::DOWNLOAD_URL;
use super::dialog::{APP_ICON, CLOSE, DOWNLOAD, Next, main_icon, message_box_text, next};
use super::version::{MINIMUM, Verdict, Version, verdict};
use super::words::{EN, WIDTH, Words};

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
            match next(&show(&EN)) {
                Next::Download => open_download_page(),
                Next::Close => {}
                Next::MessageBox => message_box(&EN),
            }
            true
        }
    }
}

/// The exe's module handle and the icon resource the dialog names, when the exe holds it.
fn app_icon() -> Option<(HMODULE, u16)> {
    // SAFETY: a plain call; `None` asks for this process's exe, which stays loaded.
    let module = unsafe { GetModuleHandleW(None) }.ok()?;
    let id = std::ptr::without_provenance::<u16>(usize::from(APP_ICON));
    // SAFETY: `module` is this exe's handle; the name and type are integer resource ids.
    let found = unsafe { FindResourceW(Some(module), PCWSTR(id), RT_GROUP_ICON) };
    main_icon(!found.0.is_null()).map(|icon| (module, icon))
}

/// Shows the dialog and returns the id of the button that closed it, or the failure of
/// `TaskDialogIndirect`.
fn show(words: &Words) -> windows::core::Result<i32> {
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
    // Clauseo's icon as the main and title-bar icon, only when the exe holds it.
    let (module, icon) = match app_icon() {
        Some((module, id)) => (
            HINSTANCE(module.0),
            PCWSTR(std::ptr::without_provenance::<u16>(usize::from(id))),
        ),
        None => (HINSTANCE::default(), PCWSTR::null()),
    };
    let config = TASKDIALOGCONFIG {
        // Both always fit in a u32.
        cbSize: u32::try_from(size_of::<TASKDIALOGCONFIG>()).unwrap_or(u32::MAX),
        hInstance: module,
        Anonymous1: TASKDIALOGCONFIG_0 { pszMainIcon: icon },
        // Wide enough for the main instruction on one line (words.rs).
        cxWidth: WIDTH,
        dwFlags: TDF_ALLOW_DIALOG_CANCELLATION,
        pszWindowTitle: PCWSTR(title.as_ptr()),
        pszMainInstruction: PCWSTR(instruction.as_ptr()),
        pszContent: PCWSTR(content.as_ptr()),
        pszFooter: PCWSTR(footer.as_ptr()),
        cButtons: u32::try_from(buttons.len()).unwrap_or(u32::MAX),
        pButtons: buttons.as_ptr(),
        nDefaultButton: DOWNLOAD,
        ..TASKDIALOGCONFIG::default()
    };
    let mut pressed = 0_i32;
    // SAFETY: `config` and every string and button it points to live until the call returns;
    // `pressed` is a valid out pointer. comctl32 6 (the app manifest) has TaskDialogIndirect.
    unsafe { TaskDialogIndirect(&raw const config, Some(&raw mut pressed), None, None) }?;
    Ok(pressed)
}

/// The plain fallback when the task dialog failed: the title, the main line and the body, with
/// an OK button. Best effort: if it fails too, the app exits as it would anyway.
fn message_box(words: &Words) {
    let title = HSTRING::from(words.title);
    let text = HSTRING::from(message_box_text(words));
    // SAFETY: both strings are valid null-terminated wide strings that outlive the call; no
    // owner window.
    let _ = unsafe {
        MessageBoxW(
            None,
            PCWSTR(text.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONWARNING,
        )
    };
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
