//! The windows' sizes, and the colour their first frame paints before the page draws.
//!
//! Contract:
//! - The window opens at [`WINDOW_SIZE`], 1280 by 800 logical px, which the room needs for its
//!   three columns; below 1000 px the room scrolls the page itself. The e2e and marks builds
//!   open at it unfitted, so the speed scoreboard is the same everywhere. The release build
//!   fits it to the primary monitor's work area ([`fitted`]) and centres it.
//! - The `settings` window opens at [`SETTINGS_SIZE`], 960 by 700, fitted and centred the same
//!   way (`settings_window.rs`, 0028).
//! - window-state restores the lawyer's own size of `main` on later launches, in the release
//!   build. The e2e and marks builds deny `main` in window-state ([`state_denylist`]), so they
//!   neither restore nor save the real app's state. Every build denies `settings`: it opens at
//!   its own size, centred, each time.
//! - The first frame is painted in the room's ground, night or day as the OS says
//!   ([`paint_first_frame`]), so the webview is never white before the page draws.

use tauri::{AppHandle, Runtime, Theme, WebviewWindow};

#[cfg(test)]
mod tests;

/// The window's size in logical px: the room's three columns need 1280; the e2e and marks
/// builds open at it always, and the release build opens at it when the work area allows
/// ([`fitted`]).
pub(crate) const WINDOW_SIZE: (f64, f64) = (1280.0, 800.0);

/// The `settings` window's size in logical px: the design's settings window.
pub(crate) const SETTINGS_SIZE: (f64, f64) = (960.0, 700.0);

/// The share of the work area the release window may take, per dimension.
const FIT_SHARE: f64 = 0.9;

/// The release window's size for a primary monitor's work area `work_area` (logical px, `None`
/// when there is no monitor): [`WINDOW_SIZE`], but no more than [`FIT_SHARE`] of the work area
/// in each dimension.
pub(crate) fn fitted(work_area: Option<(f64, f64)>) -> (f64, f64) {
    fit(WINDOW_SIZE, work_area)
}

/// `size` for a primary monitor's work area `work_area` (logical px, `None` when there is no
/// monitor): no more than [`FIT_SHARE`] of the work area in each dimension.
pub(crate) fn fit(size: (f64, f64), work_area: Option<(f64, f64)>) -> (f64, f64) {
    match work_area {
        None => size,
        Some((width, height)) => (
            size.0.min(FIT_SHARE * width),
            size.1.min(FIT_SHARE * height),
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

/// The labels window-state leaves alone: `settings` in every build, which opens at
/// [`SETTINGS_SIZE`] centred each time; and `main` in the e2e and marks builds, which open at
/// [`WINDOW_SIZE`] whatever the lawyer's saved state says.
pub(crate) const fn state_denylist() -> &'static [&'static str] {
    if cfg!(feature = "e2e") {
        &[crate::MAIN_WINDOW, crate::settings_window::SETTINGS_WINDOW]
    } else {
        &[crate::settings_window::SETTINGS_WINDOW]
    }
}

/// What window-state saves and restores: everything but the decorations, since the frame is the
/// room's (`window_frame.rs`), never a saved one, and the visibility: a quit hides the windows
/// before their state is saved (`quit.rs`), and the window is built visible. The quit saves
/// with it too, on the main thread, as an `ExitRequested` quit begins ([`save_state`]): a restart
/// or an install after it ends the app without the exit event the plugin saves on.
pub(crate) fn state_flags() -> tauri_plugin_window_state::StateFlags {
    tauri_plugin_window_state::StateFlags::all()
        - tauri_plugin_window_state::StateFlags::DECORATIONS
        - tauri_plugin_window_state::StateFlags::VISIBLE
}

/// Saves the windows' state with [`state_flags`]. On the main thread only
/// (`quit::on_exit_requested`): the plugin's save holds its cache lock while it reads each
/// window, and off the main thread each read waits on the main thread, which the plugin's own
/// save at `RunEvent::Exit` can hold on that lock. A failure is logged without the plugin's text,
/// which can name the config folder.
#[cfg(not(feature = "e2e"))]
pub(crate) fn save_state<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    use tauri_plugin_window_state::AppHandleExt as _;
    if let Err(error) = app.save_window_state(state_flags()) {
        drop(error);
        tracing::warn!("the window state was not saved at quit");
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
