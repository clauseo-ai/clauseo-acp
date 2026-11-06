//! The updater in desktop (decision record 0037): the host's `Updater` over
//! `tauri-plugin-updater`, and the install once the host has stopped.
//!
//! - **Which updater** ([`with_updater`]). A build with no endpoint gives the host none, so it
//!   keeps `NoUpdater` and shows `managed` by `package`: Linux always (it updates through its
//!   package manager), debug builds, and release builds while `plugins.updater.endpoints` in
//!   `tauri.conf.json` is empty (until R23 PR 6). The plugin's own `build` would fail with
//!   `EmptyEndpoints` there, so it is never called. The `e2e` build alone takes an endpoint from
//!   `E2E_UPDATE_ENDPOINT` and trusts the throwaway test key with it, or offers a fake update
//!   from `E2E_FAKE_UPDATE` (`choice.rs`).
//! - **The adapter** (`plugin.rs`): `check` and `download` run on Tauri's runtime and hand the
//!   host a future over a oneshot; dropping it aborts the task. The download (which the plugin
//!   verifies) is saved as the one file of `updates/<version>/`.
//! - **How it was installed** (`install_kind.rs`): per machine, a read-only `.app` folder, or
//!   per user.
//! - **The install** ([`after_host`], `install.rs`): when the host stopped with
//!   `HostExit::Install`, the stopper thread calls it after the host has stopped, the log was
//!   flushed and the counts saved: the plugin's `install` (the window state was saved on the
//!   main thread as the quit began, `quit::on_exit_requested`), which
//!   verifies the bytes again. Windows exits inside it; anything else restarts the app, which
//!   reads `update.json` at the next start (`updated_from`, or `failed`).
//!
//! Nothing here logs a URL or a path (0047): a failure is logged by its kind.

#[cfg(any(test, not(target_os = "linux")))]
mod choice;
#[cfg(all(feature = "e2e", any(test, not(target_os = "linux"))))]
mod fake;
#[cfg(any(test, not(target_os = "linux")))]
pub(crate) mod install;
#[cfg(any(test, not(target_os = "linux")))]
mod install_kind;
#[cfg(any(test, not(target_os = "linux")))]
mod plugin;
#[cfg(test)]
mod tests;

use clauseo_host::{Host, HostConfig, HostExit, HostHandle, HostJoin};
use clauseo_protocol::HostError;
use clauseo_settings::Locations;
use tauri::{AppHandle, Runtime};

/// `config` with the app's version (`tauri.conf.json`'s, which the plugin compares too) and
/// this build's updater, saving its downloads under `locations`' `updates/`, or none: the
/// host's default `NoUpdater` (see the module's notes).
pub(crate) fn with_updater<R: Runtime>(
    app: &AppHandle<R>,
    config: HostConfig,
    locations: &Locations,
) -> HostConfig {
    let config = config.with_app_version(app.package_info().version.to_string());
    #[cfg(not(target_os = "linux"))]
    if let Some(updater) = choice::updater(app, locations.updates_dir()) {
        return config.with_updater(updater);
    }
    // Linux gives the host no updater, so nothing is saved under updates/.
    #[cfg(target_os = "linux")]
    let _ = locations;
    config
}

/// `started` (the outcome of `Host::start`), after setting a task on Tauri's runtime to watch how
/// the host ends: one that stopped to install an update (`HostExit::Install`, after
/// `InstallUpdate`) asks the app to exit, so the stopper thread joins it and installs
/// ([`after_host`]). Any other end leaves the app as it is.
fn watching<R: Runtime>(
    app: &AppHandle<R>,
    started: Result<(HostHandle, HostJoin), HostError>,
) -> Result<(HostHandle, HostJoin), HostError> {
    if let Ok((host, _join)) = &started {
        let host = host.clone();
        let app = app.clone();
        // Ends with the host; nothing waits for it.
        drop(tauri::async_runtime::spawn(async move {
            if matches!(host.exit().await, HostExit::Install(_)) {
                app.exit(0);
            }
        }));
    }
    started
}

/// What the stopper thread does once the host has stopped, the log was flushed and the counts
/// saved: with `HostExit::Install`, it installs (`install.rs`; the window state was saved on the
/// main thread as the quit began, `quit::on_exit_requested`), and
/// then restarts the app, whether the install worked (macOS), failed or hung past its bound.
/// Windows exits inside the plugin's install. When an agents job outlived its bound at quit
/// (`agents_stopped` false) nothing is installed and the app restarts as it is. Returns only
/// when there was nothing to install. `before_restart` runs right before the restart, after the
/// install (the stopper marks the stop ended there: tauri lets a restart past `prevent_exit`,
/// so `RunEvent::Exit`'s `quit::stop_on_exit` would otherwise wait out its bound).
pub(crate) fn after_host<R: Runtime>(
    app: &AppHandle<R>,
    ended: HostExit,
    agents_stopped: bool,
    before_restart: impl FnOnce(),
) {
    #[cfg(not(target_os = "linux"))]
    restart_after(
        install::after_host(
            ended,
            agents_stopped,
            &mut install::AppSteps::new(app),
            install::Watch::INSTALL,
        ),
        before_restart,
        || app.restart(),
    );
    // Linux never installs: its host has `NoUpdater`, which never offers one.
    #[cfg(target_os = "linux")]
    drop((app, ended, agents_stopped, before_restart));
}

/// With `After::Restart`, `before_restart` then `restart`; with `After::Exit`, neither.
#[cfg(any(test, not(target_os = "linux")))]
fn restart_after(after: install::After, before_restart: impl FnOnce(), restart: impl FnOnce()) {
    if after == install::After::Restart {
        before_restart();
        restart();
    }
}

/// Starts the host with `config`, watched ([`watching`]).
pub(crate) fn start_host<R: Runtime>(
    app: &AppHandle<R>,
    config: HostConfig,
) -> Result<(HostHandle, HostJoin), HostError> {
    watching(app, Host::start(config))
}
