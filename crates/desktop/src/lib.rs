//! The Tauri shell: one empty `main` window showing `ui/app`, with the fixed plugin set.
//!
//! Contract:
//! - No commands, events or channels yet. `build.rs` declares an `AppManifest` with no commands,
//!   and `capabilities/main.json` grants the window no permissions.
//! - Plugins are called from Rust only. Each is registered with `plugin_rust_only`, which drops the
//!   script a plugin would inject into the page (`rust_only.rs`). clippy's `disallowed-methods`
//!   (`clippy.toml`) forbids the other public ways we know of to register a plugin, grant a
//!   permission at runtime, or build or change the context (its config, assets and permissions)
//!   after `tauri-config` has checked it.
//! - The eight plugins below are the whole set. `cargo xtask check` fails on any other
//!   `tauri-plugin-*` dependency, and on the `devtools` or `protocol-asset` features of `tauri`.
//! - This blank app is the control for every memory and start-up budget. Keep it blank.

#[cfg(all(windows, feature = "e2e"))]
mod e2e;
mod rust_only;

use rust_only::PluginRustOnly;
use tauri::{App, AppHandle, Manager, Runtime, WebviewUrl, WebviewWindowBuilder};

/// Bumped by `cargo xtask buildtime` to time an edit to this crate. Nothing reads it.
pub const BUILDTIME_MARKER: u64 = 0;

/// The label of the one window. `capabilities/main.json` grants by this label
/// (`"windows": ["main"]`), so rename both together: a window whose label no capability names
/// gets no IPC at all.
const MAIN_WINDOW: &str = "main";

/// Builds the app and runs it until it exits.
pub fn run() -> tauri::Result<()> {
    #[expect(
        clippy::disallowed_methods,
        reason = "generate_context! expands to Context::new; it is the one place a context is built"
    )]
    let context = tauri::generate_context!();
    tauri::Builder::default()
        // First, so a second launch ends before any other plugin starts.
        .plugin_rust_only(tauri_plugin_single_instance::init(
            |app, _args, _cwd| {
                raise_main_window(app);
            },
        ))
        // No URL scheme yet: the scheme is not decided.
        .plugin_rust_only(tauri_plugin_deep_link::init())
        // No endpoints, and an empty `pubkey` in tauri.conf.json: it never finds or installs an
        // update.
        .plugin_rust_only(tauri_plugin_updater::Builder::new().build())
        .plugin_rust_only(tauri_plugin_window_state::Builder::new().build())
        // dialog and opener are called from Rust only; the webview gets no permission for them.
        .plugin_rust_only(tauri_plugin_dialog::init())
        .plugin_rust_only(tauri_plugin_opener::init())
        .plugin_rust_only(tauri_plugin_os::init())
        .plugin_rust_only(tauri_plugin_notification::init())
        .setup(|app| Ok(open_main_window(app)?))
        .run(context)
}

/// Builds the `main` window in code, not in `tauri.conf.json`, so builds can add webview arguments
/// here. The https scheme is fixed before the first release: changing it moves web storage. The
/// title is `productName` from `tauri.conf.json`.
fn open_main_window(app: &App) -> tauri::Result<()> {
    let title = app.package_info().name.clone();
    let builder = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title(title)
        .use_https_scheme(true);
    #[cfg(all(windows, feature = "e2e"))]
    let builder = e2e::with_debugging_port(builder)?;
    builder.build()?;
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
