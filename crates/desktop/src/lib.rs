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
//!   `Effective::load` with the system policy sources), the `PassthroughLauncher`, the
//!   refused folders of `refused_folders`, and the launch profile (`profile.rs`, built on that
//!   thread; with none when it cannot be built). Exit waits for the start, then for
//!   `HostHandle::shutdown` and `HostJoin::join`, on their own thread, then exits.
//! - On Windows, `run` first checks the `WebView2` runtime; when it is missing or older than
//!   `minimumWebview2Version`, a task dialog says so and `run` returns without building the app
//!   (`webview2.rs`).
//! - The host's pre-write guard hook runs this executable with `--pre-write-guard`, which `main`
//!   answers before anything starts ([`guard_requested`], `pre_write_guard.rs`).
//! - Every other start first asks whether it runs inside the agent sandbox ([`launch`],
//!   `jailed.rs`): inside, it prints one line and exits with [`SANDBOXED_EXIT`] before `run`,
//!   so no window, no single-instance hand-off and no settings read.
//! - Sixteen async commands (`commands.rs`), each granted by its own permission in
//!   `capabilities/main.json` (`count_ui` in `capabilities/counts.json`, `main` only) and
//!   listed in `build.rs`'s `AppManifest`. Beside them that file
//!   grants only the two window commands of Tauri's drag script, and `window_frame.rs` adds
//!   the caption buttons' four on Windows and Linux (0015). `capabilities/settings.json`
//!   grants the settings window seven of the commands (`dispatch`, `query`, `open_stream`,
//!   `subscribe_app`, `ack`, `display_info`, `setup`) and the same drag pair, and the bridge admits
//!   from it only `SetUiSettings`, `SetToolSettings`, `SetPrivacySettings` and `GetSettings`
//!   (and `RefreshLimits`, and the updater's `SetUpdateMode`, `CheckForUpdate` and
//!   `GetUpdate`, 0037), and of the app feed
//!   only `settings_changed`: never the matter and thread lists (0028). `setup` (0048,
//!   `setup.rs`) has its own allow-list per window, and its events go to both windows.
//!   Every argument is checked in Rust first, and every host call runs as `LocalDesktop`.
//!   The `e2e` build adds `perf_channel_bytes` (`e2e.rs`, granted by `capabilities/e2e.json`),
//!   and the `marks` build `marks_memory` and `marks_finish` (`marks.rs`,
//!   `capabilities/marks.json`), granted to `main` alone. `tauri.conf.json` names `main`,
//!   `settings` and `counts`: those builds add the two at setup (`e2e::grant_scoreboard`), and the
//!   default build has neither.
//! - One `ipc::Channel` per window carries its stream (`bridge/`): each message an event's JSON
//!   as the host encoded it, a setup event (0048), a fragment of either, or a stream failure,
//!   always below 8 KB (and in
//!   the `e2e` build an event with its receive time, `timed`). At most
//!   256 KB unacknowledged per window; a 5 s watchdog that fires once per stall. A window's
//!   channel lives until the window is destroyed (`Bridge::close_window`).
//!   `docs/decisions/0012-desktop-bridge.md` records the commands, the capability and the message
//!   format. Never Tauri events. Opening `main`'s or the `settings` window's channel follows the
//!   limits on it (`HostHandle::subscribe_limits`, `follows_limits`), counted and restarted like
//!   the window's other messages; a restart sends the current view only, never the account
//!   changes it missed. No other window follows the limits.
//! - The host's log file (`host_log.rs`, 0047): started in `run` before the app, its guard in
//!   managed state, flushed by the stop thread after the host stops.
//! - The anonymous usage counts (`counts.rs`, 0036): opened on the host's start thread and
//!   handed to the host (`HostConfig::with_counts`), saved and sent by one task on Tauri's
//!   runtime every 60 s and at exit, with `time_open` from the two windows' focus.
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

#[cfg(test)]
mod agent_browser_pin;
mod bridge;
mod commands;
mod connectors;
mod counts;
mod csp;
mod delete_app_data;
mod dialogs;
#[cfg(feature = "e2e")]
mod e2e;
mod frame;
mod host_log;
mod host_start;
mod jailed;
mod machine_memory;
#[cfg(feature = "marks")]
mod marks;
mod open;
mod picked;
mod pre_write_guard;
mod profile;
mod quit;
mod rust_only;
mod settings_window;
mod setup;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
mod webview2;
mod window_frame;
mod window_size;

use std::sync::Arc;
use std::sync::atomic::Ordering;

use bridge::Bridge;
use clauseo_host::{Host, HostConfig};
use clauseo_process::{PassthroughLauncher, SpawnOptions};
use clauseo_settings::{
    Effective, Locations, PlatformDirs as _, SystemDirs, SystemEnv, SystemFiles,
};
use host_start::{HostCell, HostStart};
pub use jailed::{
    AppStages, Launched, REFUSAL, SANDBOXED_EXIT, SandboxProbe, Stages, SystemProbe, launch,
};
pub use pre_write_guard::guard_requested;
use profile::DesktopProfile;
use quit::{AGENTS_STOP_WAIT, Exit, SetupStop, stop_in_order};
use rust_only::PluginRustOnly;
use tauri::{
    App, AppHandle, Manager, RunEvent, Runtime, WebviewUrl, WebviewWindowBuilder, WindowEvent,
};

/// Bumped by `cargo xtask buildtime` to time an edit to this crate. Nothing reads it.
pub const BUILDTIME_MARKER: u64 = 0;

/// What `tests/canary.rs` drives: the counts opened and ticked as the app opens and ticks them.
/// Not an interface.
#[doc(hidden)]
pub mod counts_for_tests {
    pub use crate::counts::{about, counts_on, open, open_with, tick_once};
}

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

/// Builds the app and runs it until it exits.
pub fn run() -> tauri::Result<()> {
    // On Windows, no window without a WebView2 runtime that can draw it (webview2.rs).
    if webview2::stops_start() {
        return Ok(());
    }
    // The host's log file (decision record 0047), started before the app; its guard is managed
    // with the app and flushed when the host stops (`host_log.rs`).
    let host_log = host_log::install(&SystemDirs, &SystemEnv);
    // The scoreboard's t0, before anything else the app does.
    #[cfg(feature = "e2e")]
    e2e::mark_start();
    let context = context::<tauri::Wry>();
    let builder = tauri::Builder::default().manage(host_log);
    // The Mac's app menu, with "Settings…" (settings_window.rs). Windows and Linux get none: a
    // menu would be a bar on the undecorated window.
    #[cfg(target_os = "macos")]
    let builder = builder
        .menu(settings_window::app_menu)
        .on_menu_event(|app, event| settings_window::on_menu_event(app, event.id().as_ref()));
    let builder = builder
        // First, so a second launch ends before any other plugin starts.
        .plugin_rust_only(tauri_plugin_single_instance::init(
            |app, _args, _cwd| {
                raise_main_window(app);
            },
        ))
        // No URL scheme yet: the scheme is not decided.
        .plugin_rust_only(tauri_plugin_deep_link::init());
    // No endpoints, and an empty `pubkey` in tauri.conf.json: it never finds or installs an
    // update. Linux is left out: it updates through its package manager (R23), so the plugin
    // never runs there.
    #[cfg(not(target_os = "linux"))]
    let builder = builder.plugin_rust_only(tauri_plugin_updater::Builder::new().build());
    let app = builder
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
            commands::count_ui,
            commands::setup,
            #[cfg(feature = "e2e")]
            e2e::perf_channel_bytes,
            #[cfg(feature = "marks")]
            marks::marks_memory,
            #[cfg(feature = "marks")]
            marks::marks_finish,
        ])
        // The task is short and bounded (one host command), like the bridge's others.
        .on_window_event(|window, event| {
            counts::on_window_event(window, event);
            drop(on_window_event(window, event));
        })
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
            // The setup command waits on this cell until the setup thread fills it
            // (setup/start.rs).
            let (backend, backend_start) = setup::BackendCell::new();
            // The quit shuts `Agents` down through it (`stop_host_then_exit`).
            app.manage(SetupStop(Arc::clone(&backend)));
            let setup = setup::Setup::new(backend);
            let parts = setup::SetupParts {
                start: backend_start,
                events: setup.events(),
                host: Arc::new(setup::CellHost::new(Arc::clone(&cell))),
                confirm: Arc::new(setup::NativeSetupConfirm::new(app.handle().clone())),
                opener: Arc::new(open::NativeOpener::new(app.handle().clone())),
                home: SystemDirs.home(),
                counts: Arc::new(counts::AppCounts(app.handle().clone())),
            };
            app.manage(Bridge::with_setup(cell, setup));
            app.manage(counts::CountsState::default());
            open_main_window(app)?;
            start_host(app.handle().clone(), start, locations, parts)?;
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
/// its outcome going into the bridge's cell through `start`. The launch profile is built on
/// that thread too (it runs stetdoc and reads files), and so is the machine's memory, which the
/// host's browser limit takes (`machine_memory`), before the host starts. So are the usage
/// counts, which go to the host and to `app`'s `CountsState`, and whose task starts here. So
/// does the agents' setup, from the same effective settings, on a `clauseo-setup` thread of its
/// own (`setup/start.rs`), and the data connectors with their case-law relays
/// (`connectors.rs`, R24).
fn start_host<R: Runtime>(
    app: AppHandle<R>,
    start: HostStart,
    locations: Locations,
    setup_parts: setup::SetupParts,
) -> std::io::Result<()> {
    host_start::start_on_thread(start, move || {
        let profile = DesktopProfile::build(&locations)
            .map_err(|err| tracing::warn!(%err, "no launch profile: agents start without it"))
            .ok();
        let effective = load_effective(&locations);
        let setup_read = setup::SetupStart::read(&effective, &locations);
        // The host starts refusing launches until the keys load, and those policy blocks.
        let refusals = setup_read.launch_refusals();
        let keys = setup::KeySlot::default();
        let launcher: Arc<dyn clauseo_process::Launcher> =
            Arc::new(host_launcher(SystemDirs.home()));
        let plans = Arc::new(std::sync::RwLock::new(setup::plans_of(&effective)));
        let readers = Arc::new(setup::SwappableReaders::new(
            &effective,
            launcher,
            Arc::clone(&keys),
            Arc::clone(&plans),
        ));
        let reused = setup::ReusedLogins::default();
        let reread_at = locations.clone();
        let side = setup::HostSide {
            keys: Arc::clone(&keys),
            readers: Arc::clone(&readers) as Arc<dyn setup::LeaveReaders>,
            reused: reused.clone(),
            plans: Arc::clone(&plans),
            reread: Box::new(move || load_effective(&reread_at)),
        };
        if let Err(err) = setup::spawn(setup_read, setup_parts, side) {
            tracing::warn!(%err, "no setup thread: setup commands answer internal");
        }
        let exe = std::env::current_exe().ok();
        let counts = counts::open(&locations, &effective, exe.as_deref());
        let data_dir = effective.data_dir().to_path_buf();
        let config = host_config(effective)
            .with_login_check(Arc::new(setup::DesktopLoginCheck::new(
                SystemDirs.home(),
                reused,
            )))
            .with_sign_in_again(clauseo_agents::is_sign_in_again)
            .with_limits_reader(readers);
        let config = refusals.into_iter().fold(config, |config, (agent, code)| {
            config.with_launch_refused(agent, code)
        });
        let config = connectors::with_connectors(config, app.clone(), &data_dir);
        let config = match counts {
            Some(counts) => {
                start_counts(&app, counts.clone());
                config.with_counts(counts)
            }
            None => config,
        };
        let config = machine_memory::pass_to(
            config,
            machine_memory::total_bytes,
            HostConfig::with_machine_memory,
        );
        let hook = setup::SetupHook::new(keys, plans);
        Host::start(with_profile(config, profile, hook))
    })
}

/// `config` with setup's hook (`setup/hook.rs`: the key and plan variables) over the launch
/// profile, and stetdoc's folder first on the agents' `PATH` when stetdoc was found; with no
/// profile (it could not be built) the hook adds only setup's variables. No egress proxy yet:
/// stage C's is not wired, so the browser shows `NoProxy` (0030).
fn with_profile(
    config: HostConfig,
    profile: Option<DesktopProfile>,
    hook: setup::SetupHook,
) -> HostConfig {
    let Some(profile) = profile else {
        return config.with_profile_hook(Box::new(hook));
    };
    let config = match profile.path_prepend().and_then(absolute_folder) {
        Some(folder) => config.with_agent_path_prepend(folder),
        None => config,
    };
    config.with_profile_hook(Box::new(hook.over(Box::new(profile))))
}

/// `folder` when it is absolute. The host takes the agents' `PATH` folder as given, and a
/// relative one would name a folder under each agent's working folder, so a relative folder is
/// a bug (a debug build stops on it) and a release build logs it and prepends nothing.
fn absolute_folder(folder: std::path::PathBuf) -> Option<std::path::PathBuf> {
    debug_assert!(
        folder.is_absolute(),
        "the agents' PATH folder must be absolute: {}",
        folder.display()
    );
    if folder.is_absolute() {
        Some(folder)
    } else {
        tracing::warn!(folder = %folder.display(), "a relative folder does not go on the agents' PATH");
        None
    }
}

/// Puts `counts` in `app`'s `CountsState` and starts their task, which asks the bridge's host
/// for the switch.
fn start_counts<R: Runtime>(app: &AppHandle<R>, counts: clauseo_counts::Counts) {
    let (Some(state), Some(bridge)) = (
        app.try_state::<counts::CountsState>(),
        app.try_state::<Bridge>(),
    ) else {
        return;
    };
    state.start(counts.clone());
    let cell = bridge.cell();
    state.run_ticker(|| counts::spawn_ticker(counts, counts::HttpsUpload::new(), cell));
}

/// The effective settings under `locations`, with the system policy sources.
fn load_effective(locations: &Locations) -> Effective {
    let sources = clauseo_settings::system_policy_sources();
    let sources: Vec<&dyn clauseo_settings::PolicySource> =
        sources.iter().map(AsRef::as_ref).collect();
    Effective::load(locations, &SystemFiles, &sources)
}

/// The host's config: the effective settings, the launcher that clears the environment
/// (`host_launcher`), and the refused folders.
fn host_config(effective: Effective) -> HostConfig {
    let launcher = host_launcher(SystemDirs.home());
    let config = HostConfig::new(effective, Arc::new(launcher))
        .with_refused_folders(clauseo_settings::refused_folders(&SystemDirs));
    // With no path to this executable no guard hook goes; the permission check remains.
    match std::env::current_exe() {
        Ok(exe) => config.with_pre_write_guard(exe),
        Err(_) => config,
    }
}

/// The launcher the host starts every agent and plan reader with: it clears the environment
/// (`SpawnOptions::clear_env`, as `HostConfig::new` requires), so a child gets its spec's
/// variables and nothing else, and the variables the host drops (`GIT_*`, `SSH_*`, Codex's
/// dropped names) stay dropped. `home` shows as `~` in stderr tails.
pub(crate) fn host_launcher(home: Option<std::path::PathBuf>) -> PassthroughLauncher {
    PassthroughLauncher::new(host_spawn_options(home))
}

/// [`host_launcher`]'s options: the environment cleared, `home` shown as `~`. S18's sign-in
/// launcher uses them too (`setup/start.rs`).
pub(crate) fn host_spawn_options(home: Option<std::path::PathBuf>) -> SpawnOptions {
    let spawn = SpawnOptions::new().clear_env();
    match home {
        Some(home) => spawn.home(home),
        None => spawn,
    }
}

/// Stops the usage counts' task, then, off the UI thread, the agents' jobs (`Agents::shutdown`,
/// at most [`AGENTS_STOP_WAIT`]) and the host (waits for its start, shuts it down, joins it),
/// saves the counts, then exits with `code`.
fn stop_host_then_exit<R: Runtime>(app: &AppHandle<R>, exit: Arc<Exit>, code: i32) {
    // The counts' task first, so no tick asks a host that is shutting down (`counts.rs`).
    if let Some(counts) = app.try_state::<counts::CountsState>() {
        drop(counts.stop_ticker());
    }
    let fallback = app.clone();
    let fallback_exit = Arc::clone(&exit);
    let app = app.clone();
    let cell = app.try_state::<Bridge>().map(|bridge| bridge.cell());
    let stopper = std::thread::Builder::new()
        .name("clauseo-host-stop".to_owned())
        .spawn(move || {
            // The agents' installs and sign-ins first, their children killed (#386), then the host.
            stop_in_order(
                || {
                    app.try_state::<SetupStop>().is_none_or(|setup| {
                        setup::SetupBackend::shutdown(&*setup.0, AGENTS_STOP_WAIT)
                    })
                },
                || {
                    if let Some(cell) = &cell {
                        host_start::stop(cell);
                    }
                },
            );
            // After the host, whose last counts go in too.
            if let Some(counts) = app.try_state::<counts::CountsState>() {
                counts.save();
            }
            // The log's queued lines go to the file before either exit below skips the drops.
            host_log::flush_app(&app);
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
        host_log::flush_app(&fallback);
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
