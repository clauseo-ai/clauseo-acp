//! The Tauri shell: one empty `main` window showing `ui/app`, with the fixed plugin set.
//!
//! Contract:
//! - No commands, events or channels yet. `build.rs` declares an `AppManifest` with no commands,
//!   and `capabilities/main.json` grants the window no permissions.
//! - The eight plugins below are the whole set. `cargo xtask check` fails on any other
//!   `tauri-plugin-*` dependency, and on the `devtools` or `protocol-asset` features of `tauri`.
//! - This blank app is the control for every memory and start-up budget. Keep it blank.

use tauri::{App, AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// Bumped by `cargo xtask buildtime` to time an edit to this crate. Nothing reads it.
pub const BUILDTIME_MARKER: u64 = 0;

/// The label of the one window.
const MAIN_WINDOW: &str = "main";

/// Builds the app and runs it until it exits.
pub fn run() -> tauri::Result<()> {
    tauri::Builder::default()
        // First, so a second launch ends before any other plugin starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            raise_main_window(app);
        }))
        // No URL scheme yet: the scheme is not decided.
        .plugin(tauri_plugin_deep_link::init())
        // No endpoints, and an empty `pubkey` in tauri.conf.json: it never finds or installs an update.
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        // dialog and opener are called from Rust only; the webview gets no permission for them.
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_os::init())
        .plugin(tauri_plugin_notification::init())
        .setup(|app| Ok(open_main_window(app)?))
        .run(tauri::generate_context!())
}

/// Builds the `main` window in code, not in `tauri.conf.json`, so builds can add webview arguments
/// here. The https scheme is fixed before the first release: changing it moves web storage.
fn open_main_window(app: &App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title("Clauseo")
        .use_https_scheme(true)
        .build()?;
    Ok(())
}

/// Brings the first instance's window forward when the app is launched again.
fn raise_main_window<R: Runtime>(app: &AppHandle<R>) {
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
