//! The windows' events the app answers: a focus of a window following the limits asks the host
//! for a read, a resize of `main` reads whether it is in the OS's own full screen for the page
//! (`full_screen.rs`), and a second launch brings the first instance's window forward.

mod full_screen;
#[cfg(test)]
mod tests;

use tauri::{AppHandle, Manager as _, Runtime, WindowEvent};

use crate::bridge::Bridge;
use crate::{MAIN_WINDOW, settings_window};

pub(crate) use full_screen::FullScreen;

/// Whether the window `label` follows the limits: `main` and the settings window (U12), whose
/// channels `open_stream` subscribes to them and whose focus asks for a read.
pub(crate) fn follows_limits(label: &str) -> bool {
    label == MAIN_WINDOW || label == settings_window::SETTINGS_WINDOW
}

/// A window event: the focus of a window following the limits (`follows_limits`: `main`, and
/// the `settings` window, whose Plan and usage shows them), asks the host to read the limits
/// older than 5 minutes (`Bridge::shown`), in a task, so the event loop never waits on the host:
/// a focus before the host has started waits in its task for the start, then dispatches once.
/// Focus lost, or any other window's focus, does nothing. Returns the task, for tests. A failed
/// dispatch (a failed start) is dropped: the desktop has no log yet.
pub(crate) fn on_window_event<R: Runtime>(
    window: &tauri::Window<R>,
    event: &WindowEvent,
) -> Option<tauri::async_runtime::JoinHandle<()>> {
    if !follows_limits(window.label()) || !matches!(event, WindowEvent::Focused(true)) {
        return None;
    }
    let app = window.app_handle().clone();
    Some(tauri::async_runtime::spawn(async move {
        // Managed in `setup`, before the main window exists.
        if let Some(bridge) = app.try_state::<Bridge>() {
            let _dropped = bridge.shown().await;
        }
    }))
}

/// A resize of `main`: reads whether it is in the OS's own full screen and hands the value to
/// the bridge, which sends it to the page's app feed when it changed (`FullScreen::set`).
/// Returns the value read, for tests; `None` for another window or event, or a failed read.
///
/// On macOS, tao 0.37 sends `Resized` from `windowDidResize:` and again from
/// `windowDidEnterFullScreen:` and `windowDidExitFullScreen:`, after it has set its full-screen
/// state (`set` on `windowWillEnterFullScreen:`, cleared in `windowDidExitFullScreen:`), so the
/// last `Resized` of each transition reads the value it ends on. The green button, the menu's
/// Enter Full Screen and `set_fullscreen` all pass through those delegate calls. Called on the
/// main thread, where Tauri answers the read at once.
pub(crate) fn on_main_resized<R: Runtime>(
    window: &tauri::Window<R>,
    event: &WindowEvent,
) -> Option<bool> {
    if window.label() != MAIN_WINDOW || !matches!(event, WindowEvent::Resized(_)) {
        return None;
    }
    let full = window.is_fullscreen().ok()?;
    // Managed in `setup`, before the main window exists.
    if let Some(bridge) = window.app_handle().try_state::<Bridge>() {
        bridge.window_full_screen(full);
    }
    Some(full)
}

/// Every window's event as Tauri hands it over: the counts' focus (`counts::on_window_event`),
/// then [`on_window_event`], whose task is short and bounded (one host command), like the
/// bridge's others, then [`on_main_resized`].
pub(crate) fn on_any_window_event<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    crate::counts::on_window_event(window, event);
    drop(on_window_event(window, event));
    let _read = on_main_resized(window, event);
}

/// Brings the first instance's window forward when the app is launched again.
pub(crate) fn raise_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(window) = app.get_webview_window(MAIN_WINDOW) {
        let raised = window
            .unminimize()
            .and_then(|()| window.show())
            .and_then(|()| window.set_focus());
        #[expect(
            clippy::let_underscore_must_use,
            reason = "best effort: there is no log yet, and a window that cannot be raised stays as it was"
        )]
        let _ = raised;
    }
}
