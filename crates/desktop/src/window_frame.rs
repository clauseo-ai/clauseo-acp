//! The `main` window's frame: the OS title bar (`Chrome::Os`), or, in the room build, the page's
//! toolbar as the title bar (`Chrome::Own`).
//!
//! Contract:
//! - The frame is chosen at compile time from `VITE_CLAUSEO_WINDOW`, the variable that also
//!   switches the page (`ui/app/src/window/`): `room` gives `Own`, anything else or unset `Os`.
//!   rustc tracks `option_env!`, so the crate rebuilds when it changes, and one `pnpm tauri
//!   dev` or `tauri build` sets both halves. The window is built with its frame, so nothing
//!   flashes at start. U5 removes the switch on both sides.
//! - `Own` on the Mac: an overlay title bar with no title, and the native traffic lights moved to
//!   where the toolbar's corner lights stand ([`TRAFFIC_LIGHTS`]).
//! - `Own` on Windows and Linux: no OS decorations; Tauri keeps the system shadow and the resize
//!   borders of an undecorated window. On Windows a small native window covers the page's
//!   maximise button and answers `WM_NCHITTEST` with `HTMAXBUTTON`, so hovering it shows Snap
//!   Layouts (`windows.rs`, `docs/decisions/0015-window-frame.md`).
//! - The page drags its window with the two core window permissions `capabilities/main.json`
//!   grants ([`DRAG_PERMISSIONS`]). On Windows and Linux in the room build it also minimises,
//!   maximises or restores and closes it, and asks whether it is maximised, with the four of
//!   `capabilities/frame.json` ([`CAPTION_PERMISSIONS`]), which [`grant_caption_commands`] adds
//!   at setup. Nothing here adds a command.
//! - The native controls stand at fixed places, so the page keeps its side: in the room build
//!   the toolbar stays at the window's top at every size, the caption buttons flush top right,
//!   the lights' box fixed (`responsive.css`, `data-frame="own"`; 0015).
//! - Tauri's window commands take a `label`, so they reach every window of the app: `main` must
//!   stay the only one until the record says otherwise (0015; a test counts the builders).

#[cfg(any(windows, test))]
mod caption;
pub(crate) mod geometry;
#[cfg(windows)]
mod windows;

use tauri::{Runtime, WebviewWindow, WebviewWindowBuilder};

/// Who draws the window's title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Chrome {
    /// The OS: R12's internal window, the default build until U5.
    Os,
    /// The page's toolbar: the room build.
    Own,
}

/// The value of `VITE_CLAUSEO_WINDOW` that selects the room, and with it [`Chrome::Own`].
const ROOM: &str = "room";

/// The frame `VITE_CLAUSEO_WINDOW` (`value`, `None` when unset) selects.
pub(crate) const fn chrome_from(value: Option<&str>) -> Chrome {
    match value {
        Some(value) if eq(value.as_bytes(), ROOM.as_bytes()) => Chrome::Own,
        _ => Chrome::Os,
    }
}

/// `a == b`, in a const context.
const fn eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut at = 0;
    while at < a.len() {
        if a[at] != b[at] {
            return false;
        }
        at += 1;
    }
    true
}

/// This build's frame, from `VITE_CLAUSEO_WINDOW` when the crate was compiled.
pub(crate) const CHROME: Chrome = chrome_from(option_env!("VITE_CLAUSEO_WINDOW"));

/// The core permissions `capabilities/main.json` grants for the frame, in every build: Tauri's
/// drag script answers a press on `data-tauri-drag-region` with `start_dragging` and a
/// double-click with `internal_toggle_maximize` (maximise or restore on Windows and Linux, zoom
/// on the Mac). The default build's page has no drag region. A test holds the file to this list.
#[cfg(test)]
pub(crate) const DRAG_PERMISSIONS: &[&str] = &[
    "core:window:allow-start-dragging",
    "core:window:allow-internal-toggle-maximize",
];

/// The core permissions `capabilities/frame.json` grants, for the caption buttons and the
/// maximised state that names the middle one: on Windows and Linux (its `platforms`), in the
/// room build only ([`grant_caption_commands`]). A test holds the file to this list.
#[cfg(test)]
pub(crate) const CAPTION_PERMISSIONS: &[&str] = &[
    "core:window:allow-minimize",
    "core:window:allow-toggle-maximize",
    "core:window:allow-close",
    "core:window:allow-is-maximized",
];

/// Grants `main` the caption buttons' window commands, `capabilities/frame.json`, in the room
/// build. Added at setup, as `e2e::grant_scoreboard` adds `e2e.json`: `tauri.conf.json` names
/// only `main`, so the default build never has them, and the file's `platforms` leave out the
/// Mac, whose own traffic lights need no permission.
pub(crate) fn grant_caption_commands<R: Runtime, M: tauri::Manager<R>>(
    manager: &M,
) -> tauri::Result<()> {
    if CHROME == Chrome::Own {
        add_frame_capability(manager)?;
    }
    Ok(())
}

/// Adds `capabilities/frame.json`.
#[expect(
    clippy::disallowed_methods,
    reason = "room build only: grants the caption buttons' capability file, checked by its tests"
)]
pub(crate) fn add_frame_capability<R: Runtime, M: tauri::Manager<R>>(
    manager: &M,
) -> tauri::Result<()> {
    manager.add_capability(include_str!("../capabilities/frame.json"))
}

/// Where the Mac's native traffic lights go, in logical px from the window's top left: the
/// position `traffic_light_position` takes (the close button's frame, which is wider and taller
/// than the light it draws). From the toolbar's tokens; a test checks them against the CSS.
#[cfg(any(target_os = "macos", test))]
pub(crate) const TRAFFIC_LIGHTS: (f64, f64) = geometry::traffic_lights();

/// Gives `builder` this build's frame.
pub(crate) fn with_frame<R: Runtime, M: tauri::Manager<R>>(
    builder: WebviewWindowBuilder<'_, R, M>,
) -> WebviewWindowBuilder<'_, R, M> {
    match CHROME {
        Chrome::Os => builder,
        Chrome::Own => own(builder),
    }
}

/// The overlay title bar, its title hidden, the lights in the toolbar.
#[cfg(target_os = "macos")]
fn own<R: Runtime, M: tauri::Manager<R>>(
    builder: WebviewWindowBuilder<'_, R, M>,
) -> WebviewWindowBuilder<'_, R, M> {
    let (x, y) = TRAFFIC_LIGHTS;
    builder
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .traffic_light_position(tauri::LogicalPosition::new(x, y))
}

/// No OS decorations; the shadow (on by default for an undecorated window) and the resize
/// borders stay. On Windows the page's scrollbars float over it (Fluent overlay, `WebView2` 125 and
/// later), so a scrolling page keeps its full width and its caption buttons stay under the native
/// window placed from the client width.
#[cfg(not(target_os = "macos"))]
fn own<R: Runtime, M: tauri::Manager<R>>(
    builder: WebviewWindowBuilder<'_, R, M>,
) -> WebviewWindowBuilder<'_, R, M> {
    builder
        .decorations(false)
        .shadow(true)
        .scroll_bar_style(tauri::webview::ScrollBarStyle::FluentOverlay)
}

/// What the frame needs once the window exists: on Windows with [`Chrome::Own`], the native
/// window over the maximise button. Best effort: without it the button still works from the
/// page, with no Snap Layouts.
#[cfg(windows)]
pub(crate) fn after_build<R: Runtime>(window: &WebviewWindow<R>) {
    if CHROME == Chrome::Own {
        windows::cover_maximise(window);
    }
}

/// Nothing to do once the window exists, off Windows.
#[cfg(not(windows))]
pub(crate) fn after_build<R: Runtime>(_window: &WebviewWindow<R>) {}

#[cfg(test)]
mod tests;
