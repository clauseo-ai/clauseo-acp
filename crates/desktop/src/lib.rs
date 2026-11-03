//! The Tauri shell: the `main` window showing `ui/app`, the `settings` window showing its
//! `settings.html` when opened, the fixed plugin set, and the bridge from the windows' commands
//! to the session host (`clauseo-host`).
//!
//! Contract:
//! - Two windows, both built in code (`tauri.conf.json` has none): `main` at setup, and
//!   `settings` on `open_settings` or, on the Mac, the app menu's "Settings…" on Cmd+,
//!   (`settings_window.rs`, `docs/decisions/0028-settings-window.md`). `settings` is made on
//!   open and destroyed on close; a window's destruction drops its stream, and `main`'s
//!   destroys `settings`, so closing `main` still ends the app.
//! - Setup opens the window first, then starts the host on a thread of its own
//!   (`host_start.rs`), so the webview never waits for the store; a command sent before the host
//!   is ready waits for it, and after a failed start gets that start's error. The host runs with
//!   the effective settings (`Locations::resolve`, so `CLAUSEO_DATA_DIR` moves the data folder;
//!   `Effective::load` with the system policy sources), the `PassthroughLauncher`, and the
//!   refused folders of `refused_folders`. Exit waits for the start, then for
//!   `HostHandle::shutdown` and `HostJoin::join`, on their own thread, then exits.
//! - On Windows, `run` first checks the `WebView2` runtime; when it is missing or older than
//!   `minimumWebview2Version`, a task dialog says so and `run` returns without building the app
//!   (`webview2.rs`).
//! - The host's pre-write guard hook runs the `clauseo-guard` sidecar beside this executable
//!   (`clauseo_host::pre_write_guard::guard_program`), else this executable, with
//!   `--pre-write-guard`, which `main` then answers before anything starts
//!   (`clauseo_host::pre_write_guard::main_entry`, the sidecar's own `main`).
//! - Fourteen async commands (`commands.rs`), each granted by its own permission in
//!   `capabilities/main.json` and listed in `build.rs`'s `AppManifest`. Beside them that file
//!   grants only the two window commands of Tauri's drag script, and `window_frame.rs` adds
//!   the caption buttons' four on Windows and Linux (0015). `capabilities/settings.json`
//!   grants the settings window six of the commands (`dispatch`, `query`, `open_stream`,
//!   `subscribe_app`, `ack`, `display_info`) and the same drag pair, and the bridge admits
//!   from it only `SetUiSettings` and `GetSettings`, and of the app feed only
//!   `settings_changed`: never the matter and thread lists (0028).
//!   Every argument is checked in Rust first, and every host call runs as `LocalDesktop`.
//!   The `e2e` build adds `perf_channel_bytes` (`e2e.rs`, granted by `capabilities/e2e.json`),
//!   and the `marks` build `marks_memory` and `marks_finish` (`marks.rs`,
//!   `capabilities/marks.json`), granted to `main` alone. `tauri.conf.json` names `main`
//!   and `settings`: those builds add the two at setup (`e2e::grant_scoreboard`), and the
//!   default build has neither.
//! - One `ipc::Channel` per window carries its stream (`bridge/`): each message an event's JSON
//!   as the host encoded it, a fragment of one, or a stream failure, always below 8 KB (and in
//!   the `e2e` build an event with its receive time, `timed`). At most
//!   256 KB unacknowledged per window; a 5 s watchdog that fires once per stall. A window's
//!   channel lives until the window is destroyed (`Bridge::close_window`).
//!   `docs/decisions/0012-desktop-bridge.md` records the commands, the capability and the message
//!   format. Never Tauri events. Opening `main`'s or the `settings` window's channel follows the
//!   limits on it (`HostHandle::subscribe_limits`, `follows_limits`), counted and restarted like
//!   the window's other messages; a restart sends the current view only, never the account
//!   changes it missed. No other window follows the limits.
//! - The native focus of `main` or the `settings` window dispatches
//!   `RefreshLimits { agent: None, why: Shown }` from Rust; the host throttles it.
//! - Both windows' scripts carry `app.security.csp` like its page, the colouring worker's
//!   included: Tauri sends the header with `.html` only (`csp.rs`).
//! - No command takes a path: folders come from the native dialog drawn in Rust (`dialogs.rs`),
//!   files are named by the refs the host issued, and `reveal` names a matter or the app data
//!   folder (`open.rs`, 0026).
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
mod csp;
mod dialogs;
#[cfg(feature = "e2e")]
mod e2e;
mod frame;
mod host_start;
#[cfg(feature = "marks")]
mod marks;
mod open;
mod picked;
mod rust_only;
mod settings_window;
#[cfg(test)]
mod test_support;
mod webview2;
mod window_frame;
mod window_size;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use bridge::Bridge;
use clauseo_host::{Host, HostConfig};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_settings::{
    Effective, Locations, PlatformDirs as _, SystemDirs, SystemEnv, SystemFiles,
};
use host_start::{HostCell, HostStart};
use rust_only::PluginRustOnly;
use tauri::{
    App, AppHandle, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

/// Bumped by `cargo xtask buildtime` to time an edit to this crate. Nothing reads it.
pub const BUILDTIME_MARKER: u64 = 0;

/// The label of the main window. `capabilities/main.json` grants by this label
/// (`"windows": ["main"]`), so rename both together: a window whose label no capability names
/// gets no IPC at all.
pub(crate) const MAIN_WINDOW: &str = "main";

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

/// Builds the app and runs it until it exits.
pub fn run() -> tauri::Result<()> {
    // On Windows, no window without a WebView2 runtime that can draw it (webview2.rs).
    if webview2::stops_start() {
        return Ok(());
    }
    // The scoreboard's t0, before anything else the app does.
    #[cfg(feature = "e2e")]
    e2e::mark_start();
    let context = context::<tauri::Wry>();
    let builder = tauri::Builder::default();
    // The Mac's app menu, with "Settings…" (settings_window.rs). Windows and Linux get none: a
    // menu would be a bar on the undecorated window.
    #[cfg(target_os = "macos")]
    let builder = builder
        .menu(settings_window::app_menu)
        .on_menu_event(|app, event| settings_window::on_menu_event(app, event.id().as_ref()));
    let app = builder
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
        // Not decorations: the frame is the room's (window_frame.rs), never a saved one.
        .plugin_rust_only(
            tauri_plugin_window_state::Builder::new()
                .with_state_flags(
                    tauri_plugin_window_state::StateFlags::all()
                        - tauri_plugin_window_state::StateFlags::DECORATIONS,
                )
                .with_denylist(window_size::state_denylist())
                .build(),
        )
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
            commands::subscribe_app,
            commands::ack,
            commands::answer_permission,
            commands::pick_folder,
            commands::display_info,
            commands::open_external,
            commands::open_file,
            commands::reveal,
            commands::open_settings,
            #[cfg(feature = "e2e")]
            e2e::perf_channel_bytes,
            #[cfg(feature = "marks")]
            marks::marks_memory,
            #[cfg(feature = "marks")]
            marks::marks_finish,
        ])
        // The task is short and bounded (one host command), like the bridge's others.
        .on_window_event(|window, event| drop(on_window_event(window, event)))
        .setup(|app| {
            // The scoreboard commands' capabilities, in the e2e and marks builds only.
            #[cfg(feature = "e2e")]
            e2e::grant_scoreboard(app)?;
            // The caption buttons' window commands, on Windows and Linux.
            window_frame::grant_caption_commands(app)?;
            let locations = Locations::resolve(&SystemDirs, &SystemEnv)?;
            // Managed before the window opens: a command reaching unmanaged state fails.
            app.manage(open::AppData(locations.data_dir().to_path_buf()));
            app.manage(open::OpenGates::default());
            let (cell, start) = HostCell::new();
            app.manage(Bridge::new(cell));
            open_main_window(app)?;
            start_host(start, locations)?;
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

/// Starts the session host on its own thread (`Host::start` blocks until the store opens),
/// its outcome going into the bridge's cell through `start`.
fn start_host(start: HostStart, locations: Locations) -> std::io::Result<()> {
    host_start::start_on_thread(start, move || Host::start(host_config(&locations)))
}

/// The host's config: the effective settings under `locations` with the system policy
/// sources, the `PassthroughLauncher`, and the refused folders.
fn host_config(locations: &Locations) -> HostConfig {
    let sources = clauseo_settings::system_policy_sources();
    let sources: Vec<&dyn clauseo_settings::PolicySource> =
        sources.iter().map(AsRef::as_ref).collect();
    let effective = Effective::load(locations, &SystemFiles, &sources);
    let mut spawn = SpawnOptions::new();
    if let Some(home) = SystemDirs.home() {
        spawn = spawn.home(home);
    }
    let launcher = PassthroughLauncher::new(spawn);
    let config = HostConfig::new(effective, Arc::new(launcher))
        .with_refused_folders(clauseo_settings::refused_folders(&SystemDirs));
    // The sidecar beside this executable, else this executable in guard mode. With no path to
    // this executable no guard hook goes; the permission check remains.
    match std::env::current_exe() {
        Ok(exe) => config.with_pre_write_guard(clauseo_host::pre_write_guard::guard_program(&exe)),
        Err(_) => config,
    }
}

/// Stops the host off the UI thread (waits for its start, shuts it down, joins it), then exits
/// with `code`.
fn stop_host_then_exit<R: Runtime>(app: &AppHandle<R>, exit: Arc<Exit>, code: i32) {
    let fallback = app.clone();
    let fallback_exit = Arc::clone(&exit);
    let app = app.clone();
    let cell = app.try_state::<Bridge>().map(|bridge| bridge.cell());
    let stopper = std::thread::Builder::new()
        .name("clauseo-host-stop".to_owned())
        .spawn(move || {
            if let Some(cell) = cell {
                host_start::stop(&cell);
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
/// here, and give it the page's frame (`window_frame.rs`) and size (`window_size.rs`). The https
/// scheme is fixed before the first release: changing it moves web storage. The title is
/// `productName` from `tauri.conf.json`.
fn open_main_window(app: &App) -> tauri::Result<()> {
    let title = app.package_info().name.clone();
    // The e2e and marks builds open at WINDOW_SIZE whatever the work area; the release build
    // fits it to the primary monitor.
    let fitted = window_size::fitted(window_size::primary_work_area(app.handle()));
    let (width, height) = if cfg!(feature = "e2e") {
        window_size::WINDOW_SIZE
    } else {
        fitted
    };
    let builder = WebviewWindowBuilder::new(app, MAIN_WINDOW, WebviewUrl::App("index.html".into()))
        .title(title)
        .use_https_scheme(true)
        .inner_size(width, height)
        .center();
    let builder = window_frame::with_frame(builder);
    // Tauri sends the CSP with the page only; the scripts, the colouring worker's among them,
    // get the same policy here (csp.rs).
    let builder = csp::on_scripts(builder, app);
    #[cfg(all(windows, feature = "e2e"))]
    let builder = e2e::with_debugging_port(builder);
    // The page's perf globals, before any page script.
    #[cfg(feature = "e2e")]
    let builder = builder.initialization_script(e2e::init_script());
    let built = settings_window::number_build(app);
    let window = builder.build()?;
    window_size::paint_first_frame(&window);
    window_frame::after_build(&window);
    // Destroyed: the bridge drops main's stream, and the settings window goes too.
    settings_window::on_destroyed(&window, built);
    Ok(())
}

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
fn on_window_event<R: Runtime>(
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
