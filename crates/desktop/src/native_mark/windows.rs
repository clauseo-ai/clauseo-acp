//! Windows: the native mark's hook, subclass and paint (`native_mark.rs`).
//!
//! The comma is a filled path in GDI+, the one Windows drawing API here that anti-aliases a
//! Bézier outline in a few flat calls: plain GDI's `FillPath` leaves stair steps, and Direct2D
//! needs a factory, a render target and a geometry sink for one static shape. Everything is best
//! effort: a failed call leaves the plain ground, as before.
#![expect(
    unsafe_code,
    reason = "FFI to Win32: a thread CBT hook, comctl32 subclassing, GDI and GDI+"
)]

use std::cell::Cell;
use std::ptr;

use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, EndPaint, FillRect, HDC, PAINTSTRUCT,
};
use windows::Win32::Graphics::GdiPlus::{
    self as gdip, FillModeWinding, GdipAddPathBeziers, GdipClosePathFigure, GdipCreateFromHDC,
    GdipCreatePath, GdipCreateSolidFill, GdipDeleteBrush, GdipDeleteGraphics, GdipDeletePath,
    GdipFillPath, GdipSetPixelOffsetMode, GdipSetSmoothingMode, GdiplusShutdown, GdiplusStartup,
    GdiplusStartupInput, GpGraphics, GpPath, GpSolidFill, PixelOffsetModeHalf, PointF,
    SmoothingModeAntiAlias,
};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, GetClassNameW, GetClientRect, HCBT_CREATEWND, SetWindowsHookExW,
    UnhookWindowsHookEx, WH_CBT, WM_ERASEBKGND, WM_NCDESTROY, WM_PAINT,
};

use super::{colours, outline, placement};

/// Our subclass of the main window, among the others on it (`window_frame/windows.rs` uses
/// `0x4831_3930`, Tauri's resize borders `WM_USER + 1`).
const SUBCLASS_ID: usize = 0x4E4D_4B31;

/// The mark's state on the main thread, where the hook, the subclass and the removal all run.
#[derive(Clone, Copy)]
struct State {
    /// Windows' app theme is dark: read before the build, for the subclass's colours.
    dark: bool,
    /// The subclassed window, as an address; 0 when none.
    window: usize,
    /// GDI+'s token from the first paint to the removal; 0 when it is not started.
    gdiplus: usize,
    /// The first paint was logged.
    painted: bool,
}

thread_local! {
    static STATE: Cell<State> = const {
        Cell::new(State { dark: false, window: 0, gdiplus: 0, painted: false })
    };
}

/// The state now.
fn state() -> State {
    STATE.with(Cell::get)
}

/// Changes the state with `change`.
fn update(change: impl FnOnce(&mut State)) {
    STATE.with(|cell| {
        let mut state = cell.get();
        change(&mut state);
        cell.set(state);
    });
}

/// Runs `build` with a CBT hook on this thread that subclasses the main window as it is
/// created; the hook goes when `build` returns.
pub(super) fn through_build<T>(build: impl FnOnce() -> T) -> T {
    let dark = crate::window_size::apps_dark();
    update(|state| state.dark = dark);
    // SAFETY: `on_cbt` has HOOKPROC's signature and lives in this binary; a hook on this
    // thread's own id needs no module handle.
    let hook = unsafe { SetWindowsHookExW(WH_CBT, Some(on_cbt), None, GetCurrentThreadId()) };
    let built = build();
    if let Ok(hook) = hook {
        // SAFETY: the hook set above, unhooked once.
        if unsafe { UnhookWindowsHookEx(hook) }.is_err() {
            tracing::debug!("the native mark's hook stayed; it subclasses one window only");
        }
    }
    built
}

/// The CBT hook: subclasses the first window of the main window's class created during the
/// build, before its first message, so its show's erase draws the mark.
unsafe extern "system" fn on_cbt(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if u32::try_from(code) == Ok(HCBT_CREATEWND) && state().window == 0 {
        let window = HWND(ptr::with_exposed_provenance_mut(wparam.0));
        if is_main(window) {
            subclass(window);
        }
    }
    // SAFETY: the hook's own arguments, passed on unchanged.
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

/// Whether `window` is of the main window's class (`MAIN_WINDOW_CLASS`, which tao registers).
fn is_main(window: HWND) -> bool {
    let mut name = [0u16; 64];
    // SAFETY: `window` is being created on this thread; the slice gives the buffer's length.
    let length = unsafe { GetClassNameW(window, &mut name) };
    let length = usize::try_from(length).unwrap_or(0);
    name.get(..length).is_some_and(|name| {
        crate::MAIN_WINDOW_CLASS
            .encode_utf16()
            .eq(name.iter().copied())
    })
}

/// Subclasses `window` with [`on_message`], the theme in its reference data.
fn subclass(window: HWND) {
    let dark = usize::from(state().dark);
    // SAFETY: `window` belongs to this thread, as a thread hook sees only its own; `on_message`
    // has SUBCLASSPROC's signature and lives in this binary.
    let set = unsafe { SetWindowSubclass(window, Some(on_message), SUBCLASS_ID, dark) };
    if set.as_bool() {
        update(|state| state.window = window.0.expose_provenance());
    }
}

/// The subclass: the erase and the paint draw the ground and the mark; the window's end
/// removes the subclass. Everything else goes on to the window as it was.
unsafe extern "system" fn on_message(
    window: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    dark: usize,
) -> LRESULT {
    match msg {
        WM_ERASEBKGND => {
            draw(
                window,
                HDC(ptr::with_exposed_provenance_mut(wparam.0)),
                dark != 0,
            );
            return LRESULT(1);
        }
        WM_PAINT => {
            let mut paint = PAINTSTRUCT::default();
            // SAFETY: `window` is the window this message is for; `paint` is live through
            // EndPaint.
            let dc = unsafe { BeginPaint(window, &raw mut paint) };
            draw(window, dc, dark != 0);
            // SAFETY: ends the paint begun above, with its own PAINTSTRUCT.
            let _ended = unsafe { EndPaint(window, &raw const paint) };
            return LRESULT(0);
        }
        WM_NCDESTROY => remove_now(),
        _ => {}
    }
    // SAFETY: the subclass's own arguments, passed on to the next procedure in the chain.
    unsafe { DefSubclassProc(window, msg, wparam, lparam) }
}

/// Draws the ground over `window`'s client area on `dc`, and the comma centred on it.
fn draw(window: HWND, dc: HDC, dark: bool) {
    let mut client = RECT::default();
    // SAFETY: `window` is the live window being painted; `client` is a live RECT.
    if dc.is_invalid() || unsafe { GetClientRect(window, &raw mut client) }.is_err() {
        return;
    }
    let (ground, press) = colours(dark);
    fill_ground(dc, &client, ground);
    // SAFETY: `window` is the live window being painted.
    let dpi = unsafe { GetDpiForWindow(window) };
    let (head, tail) = outline(placement((client.right, client.bottom), dpi));
    fill_comma(dc, &head.map(point), &tail.map(point), press);
    if !state().painted {
        update(|state| state.painted = true);
        crate::startup::mark("native mark painted");
    }
}

/// A point for GDI+.
fn point((x, y): (f32, f32)) -> PointF {
    PointF { X: x, Y: y }
}

/// Fills `client` on `dc` with `ground`.
fn fill_ground(dc: HDC, client: &RECT, (r, g, b): (u8, u8, u8)) {
    let colour = COLORREF(u32::from(r) | u32::from(g) << 8 | u32::from(b) << 16);
    // SAFETY: a plain solid brush, deleted below.
    let brush = unsafe { CreateSolidBrush(colour) };
    if brush.is_invalid() {
        return;
    }
    // SAFETY: `dc` is the paint's DC, live for this message; `brush` is the one made above.
    let _filled = unsafe { FillRect(dc, client, brush) };
    // SAFETY: the brush made above, selected in no DC, deleted once.
    let _deleted = unsafe { DeleteObject(brush.into()) };
}

/// Fills the comma, `head` then `tail` as one closed figure, on `dc` in `press`, anti-aliased.
fn fill_comma(dc: HDC, head: &[PointF; 7], tail: &[PointF; 13], (r, g, b): (u8, u8, u8)) {
    if !gdiplus_started() {
        return;
    }
    let argb = 0xFF00_0000 | u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b);
    let mut graphics: *mut GpGraphics = ptr::null_mut();
    let mut path: *mut GpPath = ptr::null_mut();
    let mut brush: *mut GpSolidFill = ptr::null_mut();
    // SAFETY: `dc` is the paint's DC, live for this message; each out-pointer is a live local
    // that receives a new object, deleted below.
    let made = unsafe { GdipCreateFromHDC(dc, &raw mut graphics) } == gdip::Ok
        && unsafe { GdipCreatePath(FillModeWinding, &raw mut path) } == gdip::Ok
        && unsafe { GdipCreateSolidFill(argb, &raw mut brush) } == gdip::Ok;
    if made {
        // SAFETY: `graphics` and `path` were made above; the counts are the arrays' lengths.
        // A second run of Béziers joins the first with a straight line: the path's `L`.
        unsafe {
            GdipSetSmoothingMode(graphics, SmoothingModeAntiAlias);
            // Pixel edges at whole coordinates, as the page's CSS px are.
            GdipSetPixelOffsetMode(graphics, PixelOffsetModeHalf);
            GdipAddPathBeziers(path, head.as_ptr(), 7);
            GdipAddPathBeziers(path, tail.as_ptr(), 13);
            GdipClosePathFigure(path);
            GdipFillPath(graphics, brush.cast(), path);
        }
    }
    // SAFETY: each is null or the object made above, deleted once, the graphics last.
    unsafe {
        if !brush.is_null() {
            GdipDeleteBrush(brush.cast());
        }
        if !path.is_null() {
            GdipDeletePath(path);
        }
        if !graphics.is_null() {
            GdipDeleteGraphics(graphics);
        }
    }
}

/// Starts GDI+ for the mark the first time; whether it is started.
fn gdiplus_started() -> bool {
    if state().gdiplus != 0 {
        return true;
    }
    let input = GdiplusStartupInput {
        GdiplusVersion: 1,
        ..GdiplusStartupInput::default()
    };
    let mut token = 0usize;
    // SAFETY: `token` and `input` are live locals; the output may be null when GDI+ keeps its
    // own background thread (`SuppressBackgroundThread` false).
    let started = unsafe { GdiplusStartup(&raw mut token, &raw const input, ptr::null_mut()) };
    if started != gdip::Ok || token == 0 {
        return false;
    }
    update(|state| state.gdiplus = token);
    true
}

/// Removes the subclass from the main window and stops GDI+: the page has loaded, or the
/// window is going. Does nothing the second time. On the main thread only.
pub(super) fn remove_now() {
    let State {
        window, gdiplus, ..
    } = state();
    if window == 0 {
        return;
    }
    update(|state| {
        state.window = 0;
        state.gdiplus = 0;
    });
    let window = HWND(ptr::with_exposed_provenance_mut(window));
    // SAFETY: `window` is the window subclassed on this thread; removing a subclass from
    // inside its own procedure (WM_NCDESTROY) is allowed.
    let _removed = unsafe { RemoveWindowSubclass(window, Some(on_message), SUBCLASS_ID) };
    if gdiplus != 0 {
        // SAFETY: the token GdiplusStartup gave; every GDI+ object of the mark is deleted.
        unsafe { GdiplusShutdown(gdiplus) };
    }
    crate::startup::mark("native mark removed");
}
