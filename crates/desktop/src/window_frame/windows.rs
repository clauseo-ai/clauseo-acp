//! Windows: the native window over the page's maximise button, for Snap Layouts.
//!
//! Windows 11 shows Snap Layouts when the pointer rests on a window that answers `WM_NCHITTEST`
//! with `HTMAXBUTTON`. The page cannot answer it: wry hosts `WebView2` in a child window of its
//! own (a windowed controller), which gets the hit tests, and `WebView2`'s non-client regions
//! give only `app-region: drag` there; the maximise region needs visual hosting. So a small
//! child window of ours sits above the webview over the button, as Tauri's resize borders
//! (`TAURI_DRAG_RESIZE_BORDERS`) and Windows Terminal's drag bar do. It is layered with no
//! redirection bitmap, so it draws nothing and the page's button shows through; a child may
//! be layered from Windows 8 on, which the app manifest's `supportedOS` entries ask for.
//!
//! It answers `HTMAXBUTTON` everywhere, tells the page which look to draw (`caption.rs`), and
//! on a click posts `SC_MAXIMIZE` or `SC_RESTORE` to the window, as Windows' own button does.
//! A subclass of the top-level window moves it when the client area or the DPI changes.
//! Everything is best effort: if a call fails, the page's button still works, without Snap
//! Layouts.
#![expect(
    unsafe_code,
    reason = "FFI to the Win32 window, subclass, capture and mouse-tracking APIs"
)]

use std::cell::Cell;
use std::ptr;

use tauri::{Runtime, WebviewWindow};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    ReleaseCapture, SetCapture, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, FindWindowExW, GWLP_USERDATA, GetClientRect,
    GetMenuStringW, GetSystemMenu, GetWindowLongPtrW, HTMAXBUTTON, HWND_TOP, IDC_ARROW, IsZoomed,
    LWA_ALPHA, LoadCursorW, MF_BYCOMMAND, PostMessageW, RegisterClassExW, SC_MAXIMIZE, SC_RESTORE,
    SWP_NOACTIVATE, SWP_NOOWNERZORDER, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    SetWindowTextW, WM_CAPTURECHANGED, WM_DPICHANGED, WM_LBUTTONUP, WM_MOUSEMOVE, WM_NCDESTROY,
    WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP, WM_NCMOUSELEAVE,
    WM_NCMOUSEMOVE, WM_NCRBUTTONDBLCLK, WM_NCRBUTTONDOWN, WM_NCRBUTTONUP, WM_SIZE, WM_SYSCOMMAND,
    WNDCLASSEXW, WS_CHILD, WS_CLIPSIBLINGS, WS_EX_LAYERED, WS_EX_NOREDIRECTIONBITMAP, WS_VISIBLE,
};
use windows::core::{PCWSTR, w};

use super::caption::{Capture, Input, Look, Pointer, lparam_point, menu_word, step};
use super::geometry;

/// The native window's class.
const CLASS: PCWSTR = w!("ClauseoMaximiseButton");

/// Our subclass of the top-level window, among any others on it (Tauri's resize borders use
/// `WM_USER + 1`).
const SUBCLASS_ID: usize = 0x4831_3930;

/// The DPI at which one CSS px is one physical px.
const BASE_DPI: f64 = 96.0;

/// The native window's state, boxed in its `GWLP_USERDATA` from creation to `WM_NCDESTROY`.
/// Only shared references are ever made to it: its handlers run again inside each other
/// (`ReleaseCapture` sends `WM_CAPTURECHANGED` at once), so the pointer state is a `Cell`.
struct Button {
    /// The top-level window, which the click maximises or restores.
    top: HWND,
    /// Where the pointer stands with the button.
    pointer: Cell<Pointer>,
    /// Puts a look on the page's button.
    page: Box<dyn Fn(Look)>,
    /// Whether the window was maximised when the native window was last named, `None` before.
    named: Cell<Option<bool>>,
}

/// Covers the page's maximise button with a native window answering `HTMAXBUTTON`. Does
/// nothing when it is there already or when a call fails.
pub(super) fn cover_maximise<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(top) = window.hwnd() else {
        return;
    };
    let webview = window.clone();
    let page = move |look: Look| {
        // Best effort: a failed eval leaves the button's last look until the next one.
        drop(webview.eval(look.script()));
    };
    let button = Button {
        top,
        pointer: Cell::new(Pointer::Away),
        page: Box::new(page),
        named: Cell::new(None),
    };
    create(button);
}

/// Registers the class, makes the window over the button and subclasses the top-level window.
/// `None` when the window is there already or a call fails.
fn create(button: Button) -> Option<HWND> {
    let top = button.top;
    // SAFETY: `top` is the live window Tauri just built on this thread; CLASS is a static
    // NUL-terminated string.
    if unsafe { FindWindowExW(Some(top), None, CLASS, PCWSTR::null()) }.is_ok() {
        return None;
    }
    // SAFETY: a null name asks for this executable's module, which lives as long as the process.
    let instance = HINSTANCE(unsafe { GetModuleHandleW(PCWSTR::null()) }.ok()?.0);
    // SAFETY: IDC_ARROW names a shared system cursor; no instance is needed for it.
    let cursor = unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default();
    let class = WNDCLASSEXW {
        cbSize: u32::try_from(size_of::<WNDCLASSEXW>()).ok()?,
        lpfnWndProc: Some(button_proc),
        hInstance: instance,
        hCursor: cursor,
        lpszClassName: CLASS,
        ..WNDCLASSEXW::default()
    };
    // SAFETY: `class` is a valid WNDCLASSEXW for the call; Windows copies it. A second call
    // fails with ERROR_CLASS_ALREADY_EXISTS and keeps the same class, which is what we want.
    unsafe { RegisterClassExW(&raw const class) };
    // SAFETY: a child of `top`, on its thread, of the class just registered; no create param.
    let child = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOREDIRECTIONBITMAP,
            CLASS,
            PCWSTR::null(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            0,
            0,
            Some(top),
            None,
            Some(instance),
            None,
        )
    }
    .ok()?;
    let state = Box::into_raw(Box::new(button));
    // SAFETY: `child` is ours and alive; it holds the box until WM_NCDESTROY takes it back.
    unsafe {
        SetWindowLongPtrW(
            child,
            GWLP_USERDATA,
            state.expose_provenance().cast_signed(),
        )
    };
    // SAFETY: `child` is a layered window of ours; full alpha makes it take the pointer, and
    // with no redirection bitmap it still draws nothing.
    drop(unsafe { SetLayeredWindowAttributes(child, COLORREF(0), 255, LWA_ALPHA) });
    // SAFETY: `top` is alive on this thread; the subclass gets the child's handle as its data
    // and removes itself at WM_NCDESTROY.
    let subclassed = unsafe {
        SetWindowSubclass(
            top,
            Some(top_proc),
            SUBCLASS_ID,
            child.0.expose_provenance(),
        )
    };
    if !subclassed.as_bool() {
        // A button that cannot follow the window would cover the wrong spot: none is better.
        // SAFETY: `child` is ours; its WM_NCDESTROY frees the box.
        drop(unsafe { DestroyWindow(child) });
        return None;
    }
    // SAFETY: `top` is alive.
    place(top, child, unsafe { GetDpiForWindow(top) });
    name(child);
    Some(child)
}

/// Puts `child` over the page's maximise button in `top`'s client area at `dpi`, above the
/// webview.
fn place(top: HWND, child: HWND, dpi: u32) {
    let mut client = RECT::default();
    // SAFETY: `top` is alive (we run in its window procedure or right after building it).
    if unsafe { GetClientRect(top, &raw mut client) }.is_err() {
        return;
    }
    let rect = geometry::maximise_button(client.right - client.left, f64::from(dpi) / BASE_DPI);
    // SAFETY: both windows are alive and ours; SWP_NOACTIVATE keeps the focus where it is.
    drop(unsafe {
        SetWindowPos(
            child,
            Some(HWND_TOP),
            rect.left,
            rect.top,
            rect.width(),
            rect.height(),
            SWP_NOACTIVATE | SWP_NOOWNERZORDER,
        )
    });
}

/// Names the native window, for screen readers and UI Automation, with Windows' own word for
/// what a click does, from the top-level window's system menu: Restore while it is maximised,
/// else Maximize, in the language Windows speaks (the page's words have no Hindi for the OS's
/// command, and new English ones would be new interface strings). Only when the state changed
/// since the last name. Empty when the menu or its item is missing.
fn name(child: HWND) {
    with_button(child, |button| {
        // SAFETY: `button.top` is the live top-level window that owns `child`.
        let zoomed = unsafe { IsZoomed(button.top) }.as_bool();
        if button.named.get() == Some(zoomed) {
            return;
        }
        button.named.set(Some(zoomed));
        let command = if zoomed { SC_RESTORE } else { SC_MAXIMIZE };
        let word = menu_item(button.top, command).map_or_else(String::new, |item| menu_word(&item));
        let wide: Vec<u16> = word.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call; `child` is ours and alive.
        drop(unsafe { SetWindowTextW(child, PCWSTR(wide.as_ptr())) });
    });
}

/// The text of `command`'s item in `top`'s system menu, as the menu has it.
fn menu_item(top: HWND, command: u32) -> Option<String> {
    // SAFETY: `top` is alive; without revert the call hands back the window's own menu copy
    // (making it on first use) and changes nothing.
    let menu = unsafe { GetSystemMenu(top, false) };
    if menu.0.is_null() {
        return None;
    }
    let mut text = [0u16; 128];
    // SAFETY: `menu` is the live system menu; the slice bounds what Windows writes.
    let len = unsafe { GetMenuStringW(menu, command, Some(&mut text), MF_BYCOMMAND) };
    let len = usize::try_from(len).ok().filter(|&len| len > 0)?;
    Some(String::from_utf16_lossy(text.get(..len)?))
}

/// The top-level window's subclass: keeps the native window over the button. `data` is the
/// native window's handle.
unsafe extern "system" fn top_proc(
    top: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    data: usize,
) -> LRESULT {
    let child = HWND(ptr::with_exposed_provenance_mut(data));
    match msg {
        WM_SIZE => {
            // SAFETY: `top` is alive while it handles a message.
            place(top, child, unsafe { GetDpiForWindow(top) });
            name(child);
        }
        WM_DPICHANGED => {
            // Tao resizes the window for the new DPI first; then the button goes where the
            // new scale puts it. The new DPI is the low word of `wparam`.
            // SAFETY: passes the message on down the subclass chain, as received.
            let result = unsafe { DefSubclassProc(top, msg, wparam, lparam) };
            place(top, child, u32::try_from(wparam.0 & 0xFFFF).unwrap_or(0));
            return result;
        }
        WM_NCDESTROY => {
            // SAFETY: removes the subclass this module installed with the same proc and id.
            let _removed = unsafe { RemoveWindowSubclass(top, Some(top_proc), SUBCLASS_ID) };
        }
        _ => {}
    }
    // SAFETY: passes the message on down the subclass chain, as received.
    unsafe { DefSubclassProc(top, msg, wparam, lparam) }
}

/// The native window's procedure.
unsafe extern "system" fn button_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let input = match msg {
        WM_NCHITTEST => return LRESULT(isize::try_from(HTMAXBUTTON).unwrap_or(0)),
        WM_NCMOUSEMOVE => Input::Move,
        WM_NCMOUSELEAVE => Input::Leave,
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => Input::Down,
        WM_MOUSEMOVE => Input::Drag {
            over: over(hwnd, lparam),
        },
        WM_LBUTTONUP => Input::Up {
            over: over(hwnd, lparam),
        },
        WM_CAPTURECHANGED => Input::CaptureLost,
        // Never DefWindowProc's caption-button tracking or menu, which would act on this child
        // window rather than on the top-level one.
        WM_NCLBUTTONUP | WM_NCRBUTTONDOWN | WM_NCRBUTTONUP | WM_NCRBUTTONDBLCLK => {
            return LRESULT(0);
        }
        WM_NCDESTROY => {
            free(hwnd);
            // SAFETY: the default handling of the message, as received.
            return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };
        }
        // SAFETY: the default handling of the message, as received.
        _ => return unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    };
    act(hwnd, input);
    LRESULT(0)
}

/// Whether a client point in `lparam` is over the native window `hwnd`.
fn over(hwnd: HWND, lparam: LPARAM) -> bool {
    let (x, y) = lparam_point(lparam.0);
    let mut client = RECT::default();
    // SAFETY: `hwnd` is alive while it handles a message.
    if unsafe { GetClientRect(hwnd, &raw mut client) }.is_err() {
        return false;
    }
    geometry::Rect {
        left: client.left,
        top: client.top,
        right: client.right,
        bottom: client.bottom,
    }
    .contains(x, y)
}

/// Runs one input through the pointer states and does what the step says.
fn act(hwnd: HWND, input: Input) {
    with_button(hwnd, |button| act_on(hwnd, button, input));
}

/// Calls `f` with the native window's state, if it still has it.
fn with_button(hwnd: HWND, f: impl FnOnce(&Button)) {
    // SAFETY: GWLP_USERDATA holds our boxed `Button` from creation until WM_NCDESTROY (or 0),
    // and nothing `f` does destroys the window.
    let raw = unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) };
    let state: *const Button = ptr::with_exposed_provenance(raw.cast_unsigned());
    // SAFETY: as above; only shared references to the box are ever made.
    if let Some(button) = unsafe { state.as_ref() } {
        f(button);
    }
}

/// Does what `input`'s step says, for the native window `hwnd` and its state.
fn act_on(hwnd: HWND, button: &Button, input: Input) {
    let step = step(button.pointer.get(), input);
    button.pointer.set(step.next);
    if step.track_leave {
        let mut track = TRACKMOUSEEVENT {
            cbSize: u32::try_from(size_of::<TRACKMOUSEEVENT>()).unwrap_or(0),
            dwFlags: TME_LEAVE | TME_NONCLIENT,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        // SAFETY: `track` is a valid TRACKMOUSEEVENT naming our live window.
        drop(unsafe { TrackMouseEvent(&raw mut track) });
    }
    if step.capture == Capture::Take {
        // SAFETY: `hwnd` is ours, on this thread, and the left button is down.
        unsafe { SetCapture(hwnd) };
    }
    if let Some(look) = step.look {
        (button.page)(look);
    }
    if step.toggle {
        // SAFETY: `button.top` is the live top-level window that owns `hwnd`.
        let command = if unsafe { IsZoomed(button.top) }.as_bool() {
            SC_RESTORE
        } else {
            SC_MAXIMIZE
        };
        // Posted, not sent: the window resizes after this handler, not inside it.
        // SAFETY: as above; WM_SYSCOMMAND carries no pointer.
        drop(unsafe {
            PostMessageW(
                Some(button.top),
                WM_SYSCOMMAND,
                WPARAM(usize::try_from(command).unwrap_or(0)),
                LPARAM(0),
            )
        });
    }
    if step.capture == Capture::Release {
        // Last: it sends WM_CAPTURECHANGED here at once, which finds the state already stored.
        // SAFETY: releases the capture this thread holds.
        drop(unsafe { ReleaseCapture() });
    }
}

/// Takes the boxed state back from `hwnd` and drops it.
fn free(hwnd: HWND) {
    // SAFETY: `hwnd` is ours and in WM_NCDESTROY; swapping in 0 means nothing reads the box
    // after it is dropped.
    let raw = unsafe { SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0) };
    let state: *mut Button = ptr::with_exposed_provenance_mut(raw.cast_unsigned());
    if !state.is_null() {
        // SAFETY: `state` came from Box::into_raw in `create` and is taken back only here.
        drop(unsafe { Box::from_raw(state) });
    }
}
