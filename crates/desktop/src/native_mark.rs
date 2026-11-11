//! The room's quiet mark in the main window on Windows, from the window's first frame through
//! `WebView2`'s start, which can take 4 to 10 s after a reboot or on a first-ever open.
//!
//! Contract:
//! - The window's own background shows the comma of `ui/app/index.html`'s first paint at the
//!   same size (40 CSS px wide at the window's DPI) and centred in the client area, in
//!   `--ground-press` on `--ground` of Windows' app theme ([`colours`]; `window_size.rs` reads
//!   the theme). It is static: breathing would need a timer on the thread that `WebView2`'s
//!   build holds.
//! - A CBT hook on this thread, set for the main window's build only ([`through_build`]),
//!   subclasses the window as tao creates it, before tao shows it. So the show's synchronous
//!   erase paints the mark before wry creates its container window and starts `WebView2`, whose
//!   container draws nothing over it. wry's creation waits pump messages (webview2-com's
//!   `wait_with_pump`), so the window repaints during the wait and Windows does not ghost it as
//!   not responding.
//! - The subclass goes at the page's first load ([`remove`]), or with the window: `WebView2`
//!   covers the client area once its controller exists, and the page's own mark takes over.
//! - host.log (`startup.rs`): `start-up: native mark painted` the first time,
//!   `start-up: native mark removed` once.
//! - Elsewhere nothing: macOS and Linux build the webview with the window, with no wait.

use tauri::{Runtime, WebviewWindow};

#[cfg(windows)]
mod windows;

#[cfg(test)]
mod tests;

/// `build`, the main window's build, with the mark from the window's first frame.
#[cfg(windows)]
pub(crate) fn through_build<T>(build: impl FnOnce() -> T) -> T {
    windows::through_build(build)
}

/// `build`, the main window's build, as it is: the mark is Windows' only.
#[cfg(not(windows))]
pub(crate) fn through_build<T>(build: impl FnOnce() -> T) -> T {
    build()
}

/// Removes the mark's subclass from the main window, on the main thread: the page has loaded.
/// Does nothing when it is gone already, and elsewhere than Windows.
#[cfg(windows)]
pub(crate) fn remove<R: Runtime>(window: &WebviewWindow<R>) {
    if window.run_on_main_thread(windows::remove_now).is_err() {
        // The window's WM_NCDESTROY removes it then.
        tracing::debug!("the native mark's removal did not reach the main thread");
    }
}

/// Removes the mark's subclass from the main window: nothing elsewhere than Windows.
#[cfg(not(windows))]
pub(crate) fn remove<R: Runtime>(_window: &WebviewWindow<R>) {}

/// The day `--ground-press`, `ui/components/src/styles/tokens-colour.css`: the first paint's
/// comma in `index.html`.
#[cfg(any(windows, test))]
pub(crate) const DAY_PRESS: (u8, u8, u8) = (210, 205, 197);

/// The night `--ground-press`, `ui/components/src/styles/tokens-colour.css`.
#[cfg(any(windows, test))]
pub(crate) const NIGHT_PRESS: (u8, u8, u8) = (41, 39, 37);

/// The ground and the comma's colour when Windows' app theme is `dark` or not.
#[cfg(any(windows, test))]
pub(crate) const fn colours(dark: bool) -> ((u8, u8, u8), (u8, u8, u8)) {
    let press = if dark { NIGHT_PRESS } else { DAY_PRESS };
    (crate::window_size::ground(dark), press)
}

/// The comma's viewBox in `index.html`, in its own units: left, top, width, height.
#[cfg(any(windows, test))]
const VIEW_LEFT: f32 = 7752.0;
#[cfg(any(windows, test))]
const VIEW_TOP: f32 = 944.0;
#[cfg(any(windows, test))]
const VIEW_WIDTH: f32 = 411.0;
#[cfg(any(windows, test))]
const VIEW_HEIGHT: f32 = 695.0;

/// The comma's width in CSS px, `#first-mark svg` in `index.html`; its height follows the
/// viewBox.
#[cfg(any(windows, test))]
const MARK_WIDTH: f32 = 40.0;

/// The DPI at which one CSS px is one physical px; `WebView2` zooms the page by the window's DPI.
#[cfg(any(windows, test))]
const BASE_DPI: u16 = 96;

/// The comma's outline up to its straight edge, in viewBox units, as `index.html`'s path: its
/// `M` point, then two cubic Béziers of three points each.
#[cfg(any(windows, test))]
pub(crate) const HEAD: [(f32, f32); 7] = [
    (8162.05, 1149.65),
    (8162.05, 1188.03),
    (8151.0, 1275.08),
    (8100.24, 1368.41),
    (8035.48, 1487.55),
    (7941.48, 1552.6),
    (7819.19, 1638.22),
];

/// The rest of the outline: the straight edge's end (the path's `L`), then four cubic Béziers
/// back to the `M` point, where the path closes.
#[cfg(any(windows, test))]
pub(crate) const TAIL: [(f32, f32); 13] = [
    (7760.24, 1553.94),
    (7842.72, 1496.22),
    (7903.1, 1431.36),
    (7934.24, 1347.75),
    (7828.43, 1332.79),
    (7753.0, 1237.46),
    (7762.91, 1131.08),
    (7772.81, 1024.7),
    (7864.62, 944.89),
    (7971.38, 949.84),
    (8078.05, 954.79),
    (8162.05, 1042.79),
    (8162.05, 1149.65),
];

/// Where the comma's viewBox sits in a client area: physical px per viewBox unit, and the box's
/// top-left corner in physical px.
#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Placement {
    /// Physical px per viewBox unit.
    pub(crate) scale: f32,
    /// The box's left edge.
    pub(crate) left: f32,
    /// The box's top edge.
    pub(crate) top: f32,
}

/// The comma's place in a client area `client` (width, height) physical px at `dpi`: centred,
/// [`MARK_WIDTH`] CSS px wide. A `dpi` of 0 (a failed `GetDpiForWindow`) counts as 96.
#[cfg(any(windows, test))]
pub(crate) fn placement(client: (i32, i32), dpi: u32) -> Placement {
    let dpi = if dpi == 0 {
        BASE_DPI
    } else {
        u16::try_from(dpi).unwrap_or(u16::MAX)
    };
    let scale = MARK_WIDTH * f32::from(dpi) / f32::from(BASE_DPI) / VIEW_WIDTH;
    Placement {
        scale,
        left: (px(client.0) - VIEW_WIDTH * scale) / 2.0,
        top: (px(client.1) - VIEW_HEIGHT * scale) / 2.0,
    }
}

/// A client-area length as f32, without a lossy cast: Windows' window sizes stay far inside
/// i16; a negative length counts as 0.
#[cfg(any(windows, test))]
fn px(length: i32) -> f32 {
    f32::from(i16::try_from(length.max(0)).unwrap_or(i16::MAX))
}

/// The comma's outline: [`HEAD`]'s points, then [`TAIL`]'s.
#[cfg(any(windows, test))]
pub(crate) type Outline = ([(f32, f32); 7], [(f32, f32); 13]);

/// The comma's outline in physical px for `placement`: [`HEAD`] then [`TAIL`], each point moved
/// from viewBox units.
#[cfg(any(windows, test))]
pub(crate) fn outline(placement: Placement) -> Outline {
    let place = |(x, y): (f32, f32)| {
        (
            placement.left + (x - VIEW_LEFT) * placement.scale,
            placement.top + (y - VIEW_TOP) * placement.scale,
        )
    };
    (HEAD.map(place), TAIL.map(place))
}
