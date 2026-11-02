//! The `main` window's size, and the colour its first frame paints before the page draws.
//!
//! Contract:
//! - The window opens at [`WINDOW_SIZE`], 1280 by 800 logical px, which the room needs for its
//!   three columns; below 1000 px the room scrolls the page itself. The e2e and marks builds
//!   open at it unfitted, so the speed scoreboard is the same everywhere. The release build
//!   fits it to the primary monitor's work area ([`fitted`]) and centres it.
//! - window-state restores the lawyer's own size on later launches, in the release build. The
//!   e2e and marks builds deny `main` in window-state ([`state_denylist`]), so they neither
//!   restore nor save the real app's state.
//! - The first frame is painted in the room's ground, night or day as the OS says
//!   ([`paint_first_frame`]), so the webview is never white before the page draws.

use tauri::{AppHandle, Runtime, Theme, WebviewWindow};

#[cfg(test)]
mod tests;

/// The window's size in logical px: the room's three columns need 1280; the e2e and marks
/// builds open at it always, and the release build opens at it when the work area allows
/// ([`fitted`]).
pub(crate) const WINDOW_SIZE: (f64, f64) = (1280.0, 800.0);

/// The share of the work area the release window may take, per dimension.
const FIT_SHARE: f64 = 0.9;

/// The release window's size for a primary monitor's work area `work_area` (logical px, `None`
/// when there is no monitor): [`WINDOW_SIZE`], but no more than [`FIT_SHARE`] of the work area
/// in each dimension.
pub(crate) fn fitted(work_area: Option<(f64, f64)>) -> (f64, f64) {
    match work_area {
        None => WINDOW_SIZE,
        Some((width, height)) => (
            WINDOW_SIZE.0.min(FIT_SHARE * width),
            WINDOW_SIZE.1.min(FIT_SHARE * height),
        ),
    }
}

/// The primary monitor's work area in logical px, `None` when there is none or it cannot be
/// read.
pub(crate) fn primary_work_area<R: Runtime>(app: &AppHandle<R>) -> Option<(f64, f64)> {
    let monitor = app.primary_monitor().ok().flatten()?;
    let scale = monitor.scale_factor();
    let area = monitor.work_area().size;
    Some((
        f64::from(area.width) / scale,
        f64::from(area.height) / scale,
    ))
}

/// The labels window-state leaves alone: `main` in the e2e and marks builds, which open at
/// [`WINDOW_SIZE`] whatever the lawyer's saved state says; none in the release build.
pub(crate) const fn state_denylist() -> &'static [&'static str] {
    if cfg!(feature = "e2e") {
        &[crate::MAIN_WINDOW]
    } else {
        &[]
    }
}

/// The night ground, `--ground` in `ui/components/src/styles/tokens-colour.css`.
pub(crate) const NIGHT_GROUND: (u8, u8, u8) = (27, 26, 24);

/// The day ground, `--ground` in `ui/components/src/styles/tokens-colour.css`.
pub(crate) const DAY_GROUND: (u8, u8, u8) = (229, 226, 220);

/// The ground the first frame paints: [`NIGHT_GROUND`] when `dark`, else [`DAY_GROUND`].
pub(crate) const fn ground(dark: bool) -> (u8, u8, u8) {
    if dark { NIGHT_GROUND } else { DAY_GROUND }
}

/// Paints the window's background in the ground of the OS theme, so the first frame is not
/// white. The day ground when the theme cannot be read.
pub(crate) fn paint_first_frame<R: Runtime>(window: &WebviewWindow<R>) {
    let dark = matches!(window.theme(), Ok(Theme::Dark));
    let (r, g, b) = ground(dark);
    let color = tauri::window::Color(r, g, b, 255);
    // Best effort: a refused colour leaves the first frame white, as it was before.
    let _refused = window.set_background_color(Some(color));
}
