//! The windows' events the app answers: a focus of a window following the limits asks the host
//! for a read, and a second launch brings the first instance's window forward.

use tauri::{AppHandle, Manager as _, Runtime, WindowEvent};

use crate::bridge::Bridge;
use crate::{MAIN_WINDOW, settings_window};

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

/// Every window's event as Tauri hands it over: the counts' focus (`counts::on_window_event`),
/// then [`on_window_event`], whose task is short and bounded (one host command), like the
/// bridge's others.
pub(crate) fn on_any_window_event<R: Runtime>(window: &tauri::Window<R>, event: &WindowEvent) {
    crate::counts::on_window_event(window, event);
    drop(on_window_event(window, event));
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
