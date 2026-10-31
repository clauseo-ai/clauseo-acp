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
//!   The `e2e` build adds `perf_channel_bytes` (`e2e.rs`, granted by `capabilities/e2e.json`),
//!   and the `marks` build `marks_memory` and `marks_finish` (`marks.rs`,
//!   `capabilities/marks.json`). `tauri.conf.json` names only `main`: those builds add the
//!   two at setup (`e2e::grant_scoreboard`), and the default build has neither.
//! - One `ipc::Channel` per window carries its stream (`bridge/`): each message an event's JSON
//!   as the host encoded it, a fragment of one, or a stream failure, always below 8 KB (and in
//!   the `e2e` build an event with its receive time, `timed`). At most
//!   256 KB unacknowledged per window; a 5 s watchdog that fires once per stall.
//!   `docs/decisions/0012-desktop-bridge.md` records the commands, the capability and the message
//!   format. Never Tauri events.
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
#[cfg(feature = "marks")]
mod marks;
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

/// Where exit stands: a stopper started, and the host stopped (so the next request exits).
#[derive(Default)]
struct Exit {
    stopping: AtomicBool,
    stopped: AtomicBool,
}

/// The host thread, joined at exit.
struct HostThread(Mutex<Option<HostJoin>>);

/// Builds the app and runs it until it exits.
pub fn run() -> tauri::Result<()> {
    // The scoreboard's t0, before anything else the app does.
    #[cfg(feature = "e2e")]
    e2e::mark_start();
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
            #[cfg(feature = "e2e")]
            e2e::perf_channel_bytes,
            #[cfg(feature = "marks")]
            marks::marks_memory,
            #[cfg(feature = "marks")]
            marks::marks_finish,
        ])
        .setup(|app| {
            // The scoreboard commands' capabilities, in the e2e and marks builds only.
            #[cfg(feature = "e2e")]
            e2e::grant_scoreboard(app)?;
            start_host(app)?;
            open_main_window(app)?;
            Ok(())
        })
        .build(context)?;
    let exit = Arc::new(Exit::default());
    app.run(move |app, event| {
        if let RunEvent::ExitRequested { api, code, .. } = event
            && !exit.stopped.load(Ordering::Acquire)
        {
            api.prevent_exit();
            // One stopper: a later request (another close, the stopper's own exit racing a
            // second one) only waits for it.
            let first = exit
                .stopping
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok();
            if first {
                stop_host_then_exit(app, Arc::clone(&exit), code.unwrap_or(0));
            }
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
fn stop_host_then_exit<R: Runtime>(app: &AppHandle<R>, exit: Arc<Exit>, code: i32) {
    let fallback = app.clone();
    let fallback_exit = Arc::clone(&exit);
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
            exit.stopped.store(true, Ordering::Release);
            // The e2e build ends the process itself once the host has joined. On a Windows
            // session with no visible window (session 0, as in CI) `app.exit` never yields
            // `RunEvent::Exit`: tao's loop ends only after a paint on a visible window (tao
            // 0.37.2 `event_loop.rs:271`, `:2290`). `cleanup_before_exit` clears resources and
            // hides windows; it does not send `RunEvent::Exit`, so window-state (which saves on
            // that event only) writes nothing to the real config folder. The default build
            // keeps `app.exit`.
            #[cfg(feature = "e2e")]
            #[expect(
                clippy::exit,
                reason = "e2e build only: app.exit never ends the loop without a visible window"
            )]
            {
                app.cleanup_before_exit();
                std::process::exit(code);
            }
            #[cfg(not(feature = "e2e"))]
            app.exit(code);
        });
    if stopper.is_err() {
        // No thread for it: exit now; the host's next start repairs what it left open.
        fallback_exit.stopped.store(true, Ordering::Release);
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
    // The page's perf globals, before any page script.
    #[cfg(feature = "e2e")]
    let builder = builder.initialization_script(e2e::init_script());
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
