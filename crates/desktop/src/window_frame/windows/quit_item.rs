//! The Win32 side of the Quit item (`window_frame/quit_item.rs`): the item in the main window's
//! system menu, and a subclass of the window that quits when it is chosen. Its FFI falls under
//! the parent module's `unsafe_code` expectation (`../windows.rs`).

use std::ptr;

use clauseo_protocol::Language;
use tauri::{Manager as _, Runtime, WebviewWindow};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, GetMenuState, GetSystemMenu, HMENU, MF_BYCOMMAND, MF_SEPARATOR, MF_STRING,
    ModifyMenuW, WM_NCDESTROY, WM_SYSCOMMAND,
};
use windows::core::PCWSTR;

use crate::window_frame::quit_item::{QUIT_ID, is_quit, label, wide};

/// The Quit item's subclass of the main window, beside the maximise cover's (`0x4831_3930`)
/// and Tauri's resize borders' (`WM_USER + 1`).
const SUBCLASS_ID: usize = 0x4831_3931;

/// What choosing the item does, boxed in the subclass's data until `WM_NCDESTROY`.
type Quit = Box<dyn Fn()>;

/// Adds the separator and Quit Clauseo to `window`'s system menu, in the language the installer
/// recorded (`ui_language::saved`, else English), and the subclass that quits on it. Call it on
/// the window's thread (the main thread), once the window exists. Does nothing when the item is
/// there already or a call fails.
pub(crate) fn add<R: Runtime>(window: &WebviewWindow<R>) {
    let language = crate::ui_language::saved().unwrap_or(Language::En);
    let Ok(top) = window.hwnd() else {
        return;
    };
    let Some(menu) = system_menu(top) else {
        return;
    };
    if has_quit(menu) {
        return;
    }
    let app = window.app_handle().clone();
    let quit: Box<Quit> = Box::new(Box::new(move || app.exit(0)));
    let data = Box::into_raw(quit);
    // SAFETY: `top` is the live window Tauri just built on this thread; the subclass owns the
    // box from here and frees it at WM_NCDESTROY.
    let subclassed =
        unsafe { SetWindowSubclass(top, Some(quit_proc), SUBCLASS_ID, data.expose_provenance()) };
    if !subclassed.as_bool() {
        // No subclass, no item: one that did nothing would be worse.
        // SAFETY: `data` came from Box::into_raw just above and nothing else holds it.
        drop(unsafe { Box::from_raw(data) });
        return;
    }
    let text = wide(&label(language));
    // SAFETY: `menu` is the window's live system menu; `text` is NUL-terminated and outlives
    // the call, which copies it.
    unsafe {
        drop(AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null()));
        drop(AppendMenuW(
            menu,
            MF_STRING,
            QUIT_ID as usize,
            PCWSTR(text.as_ptr()),
        ));
    }
}

/// Renames the Quit item of `window`'s system menu in `language`. Call it on the window's
/// thread. Does nothing when the item is missing or a call fails.
pub(crate) fn relabel<R: Runtime>(window: &WebviewWindow<R>, language: Language) {
    let Ok(top) = window.hwnd() else {
        return;
    };
    let Some(menu) = system_menu(top) else {
        return;
    };
    if !has_quit(menu) {
        return;
    }
    let text = wide(&label(language));
    // SAFETY: `menu` is the window's live system menu, which holds the item; `text` is
    // NUL-terminated and outlives the call, which copies it.
    drop(unsafe {
        ModifyMenuW(
            menu,
            QUIT_ID,
            MF_BYCOMMAND | MF_STRING,
            QUIT_ID as usize,
            PCWSTR(text.as_ptr()),
        )
    });
}

/// `top`'s own system menu, `None` when it has none.
fn system_menu(top: HWND) -> Option<HMENU> {
    // SAFETY: `top` is alive; without revert the call hands back the window's own menu copy
    // (making it on first use).
    let menu = unsafe { GetSystemMenu(top, false) };
    (!menu.0.is_null()).then_some(menu)
}

/// Whether `menu` holds the Quit item.
fn has_quit(menu: HMENU) -> bool {
    // SAFETY: `menu` is a live menu; the call only reads it, and answers u32::MAX (-1) for an
    // item that is not there.
    let state = unsafe { GetMenuState(menu, QUIT_ID, MF_BYCOMMAND) };
    state != u32::MAX
}

/// The main window's subclass: quits on the Quit item. `data` is the boxed [`Quit`].
unsafe extern "system" fn quit_proc(
    top: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    match msg {
        WM_SYSCOMMAND if is_quit(wparam.0) => {
            let quit: *const Quit = ptr::with_exposed_provenance(data);
            // SAFETY: the box lives from `add` until WM_NCDESTROY; only shared references are
            // made to it.
            if let Some(quit) = unsafe { quit.as_ref() } {
                quit();
            }
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            // SAFETY: removes the subclass `add` installed with the same proc and id.
            let _removed = unsafe { RemoveWindowSubclass(top, Some(quit_proc), SUBCLASS_ID) };
            let quit: *mut Quit = ptr::with_exposed_provenance_mut(data);
            // SAFETY: `quit` came from Box::into_raw in `add`, and the subclass that held it is
            // gone, so nothing reads it again.
            drop(unsafe { Box::from_raw(quit) });
        }
        _ => {}
    }
    // SAFETY: passes the message on down the subclass chain, as received.
    unsafe { DefSubclassProc(top, msg, wparam, lparam) }
}
