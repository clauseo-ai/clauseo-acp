//! The Tauri shell: one `main` window showing `ui/app`, the fixed plugin set, and the bridge
//! from the window's commands to the session host (`clauseo-host`).
//!
//! Contract:
//! - Setup starts the host with the effective settings (`Locations::resolve`, so
//!   `CLAUSEO_DATA_DIR` moves the data folder; `Effective::load` with the system policy
//!   sources), the `PassthroughLauncher`, and the refused folders of `refused_folders`. Exit
//!   waits for `HostHandle::shutdown` and `HostJoin::join` on their own thread, then exits.
//! - Nine async commands (`commands.rs`), each granted by its own permission in
//!   `capabilities/main.json` and listed in `build.rs`'s `AppManifest`; nothing else is granted.
//!   Every argument is checked in Rust first, and every host call runs as `LocalDesktop`.
//! - One `ipc::Channel` per window carries its stream (`bridge/`): each message an event's JSON
//!   as the host encoded it, or a fragment of one, always below 8 KB. At most 256 KB
//!   unacknowledged per window; a 5 s watchdog. `docs/decisions/0012-desktop-bridge.md` records
//!   the commands, the capability and the message format. Never Tauri events.
//! - No command takes a path: folders come from the native dialog drawn in Rust (`dialogs.rs`).
//! - Plugins are called from Rust only. Each is registered with `plugin_rust_only`, which drops the
//!   script a plugin would inject into the page (`rust_only.rs`). clippy's `disallowed-methods`
//!   (`clippy.toml`) forbids the other public ways we know of to register a plugin, grant a
//!   permission at runtime, or build or change the context (its config, assets and permissions)
//!   after `tauri-config` has checked it.
//! - The eight plugins below are the whole set. `cargo xtask check` fails on any other
//!   `tauri-plugin-*` dependency, and on the `devtools` or `protocol-asset` features of `tauri`.
//! - The TypeScript bindings in `ui/app/src/transport/bindings.ts` are written by a unit test
//!   (`src/commands/bindings.rs`); tauri-specta and specta are dev-dependencies only.

mod bridge;
mod commands;
mod dialogs;
#[cfg(feature = "e2e")]
mod e2e;
mod frame;
mod rust_only;
#[cfg(test)]
mod test_support;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use bridge::Bridge;
use clauseo_host::{Host, HostConfig, HostJoin};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_settings::{
    Effective, Locations, PlatformDirs as _, SystemDirs, SystemEnv, SystemFiles,
};
use rust_only::PluginRustOnly;
use tauri::{App, AppHandle, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindowBuilder};

/// Bumped by `cargo xtask buildtime` to time an edit to this crate. Nothing reads it.
pub const BUILDTIME_MARKER: u64 = 0;

/// The label of the one window. `capabilities/main.json` grants by this label
/// (`"windows": ["main"]`), so rename both together: a window whose label no capability names
/// gets no IPC at all.
const MAIN_WINDOW: &str = "main";

/// The app's context, from the checked `tauri.conf.json` and capabilities. Generic, so the
/// mock-runtime tests run under the same capabilities as the app.
#[expect(
    clippy::disallowed_methods,
    reason = "generate_context! expands to Context::new; it is the one place a context is built"
)]
fn context<R: Runtime>() -> tauri::Context<R> {
    tauri::generate_context!()
}

/// The host thread, joined at exit.
struct HostThread(Mutex<Option<HostJoin>>);

/// Builds the app and runs it until it exits.
pub fn run() -> tauri::Result<()> {
    let context = context::<tauri::Wry>();
    let app = tauri::Builder::default()
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
        // The list of COMMANDS in commands.rs; a test checks they agree.
        .invoke_handler(tauri::generate_handler![
            commands::dispatch,
            commands::query,
            commands::open_stream,
            commands::subscribe,
            commands::unsubscribe,
            commands::ack,
            commands::answer_permission,
            commands::pick_folder,
            commands::display_info,
        ])
        .setup(|app| {
            start_host(app)?;
            open_main_window(app)?;
            Ok(())
        })
        .build(context)?;
    let stopped = Arc::new(AtomicBool::new(false));
    app.run(move |app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event
            && !stopped.load(Ordering::Acquire)
        {
            api.prevent_exit();
            stop_host_then_exit(app, Arc::clone(&stopped), code.unwrap_or(0));
        }
    });
    Ok(())
}

/// Starts the session host and hands the bridge to the commands.
fn start_host(app: &App) -> Result<(), Box<dyn std::error::Error>> {
    let locations = Locations::resolve(&SystemDirs, &SystemEnv)?;
    let sources = clauseo_settings::system_policy_sources();
    let sources: Vec<&dyn clauseo_settings::PolicySource> =
        sources.iter().map(AsRef::as_ref).collect();
    let effective = Effective::load(&locations, &SystemFiles, &sources);
    let mut spawn = SpawnOptions::new();
    if let Some(home) = SystemDirs.home() {
        spawn = spawn.home(home);
    }
    let launcher = PassthroughLauncher::new(spawn);
    let config = HostConfig::new(effective, Arc::new(launcher))
        .with_refused_folders(clauseo_settings::refused_folders(&SystemDirs));
    let (handle, join) = Host::start(config)?;
    app.manage(Bridge::new(handle));
    app.manage(HostThread(Mutex::new(Some(join))));
    Ok(())
}

/// Stops the host off the UI thread (shutdown, then join), then exits with `code`.
fn stop_host_then_exit<R: Runtime>(app: &AppHandle<R>, stopped: Arc<AtomicBool>, code: i32) {
    let fallback = app.clone();
    let fallback_stopped = Arc::clone(&stopped);
    let app = app.clone();
    let host = app
        .try_state::<Bridge>()
        .map(|bridge| bridge.host().clone());
    let join = app
        .try_state::<HostThread>()
        .and_then(|thread| bridge::lock(&thread.0).take());
    let stopper = std::thread::Builder::new()
        .name("clauseo-host-stop".to_owned())
        .spawn(move || {
            if let Some(host) = host {
                // Already stopped or failed: join reports it, and the app exits either way.
                let _stopped = futures::executor::block_on(host.shutdown());
            }
            if let Some(join) = join {
                let _joined = join.join();
            }
            stopped.store(true, Ordering::Release);
            app.exit(code);
        });
    if stopper.is_err() {
        // No thread for it: exit now; the host's next start repairs what it left open.
        fallback_stopped.store(true, Ordering::Release);
        fallback.exit(code);
    }
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
    let builder = e2e::with_debugging_port(builder);
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
