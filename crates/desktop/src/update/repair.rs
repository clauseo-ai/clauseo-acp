//! "Download and run the installer again" (the Safe workspace's card when its set-up program's
//! admin copy is missing, decision record 0037): `repair_install` fetches this exact version's
//! per-user installer for this architecture from its release, checks it as the updater checks an
//! update, and runs it, interactively, with `/CLAUSEO_REPAIR`. Only that installer can put the
//! admin copy in Program Files; its own elevated step raises the one UAC prompt.
//!
//! - **Who:** the main window only (`bridge/admit.rs`, and `capabilities/main.json`); Windows, a
//!   per-user install, a release or test build with its updater key, and no update scheduled or
//!   installing (`plan.rs`). Anything else is `repair.not_eligible`, and the card shows no button.
//! - **What:** `<releases>/download/<tag>/Clauseo_<version>_<x64|arm64>-setup.exe` and its `.sig`,
//!   `<tag>` from the build's release mode compiled in (`v` for a release, `test-v` for a test
//!   build), never the version string; the running version and architecture, so a repair never
//!   moves either. The updater's HTTPS client.
//! - **The check:** #728's `check_update_file` with the updater key of the build's mode (the
//!   config's for a release, the test key for a test build): the signature, global signature
//!   included, must name exactly that file and version, read back through the handle that holds
//!   the file until the installer has started (`flow.rs`, `windows.rs`).
//! - **The hand-over:** the `repair-pending` marker (`marker.rs`), every window destroyed (the
//!   installer refuses while the main window class exists; lib.rs holds the exit that asks for),
//!   the start, then the app's quit, bounded ([`app::EXIT_BOUND`]). The installer starts Clauseo
//!   again once at its end (stage F's `/CLAUSEO_REPAIR`), and the single-instance guard keeps it
//!   to one. A failed start brings the main window back with `repair.launch_failed`.
//! - **The next start** reads and deletes the marker ([`RepairState::at_start`]) and tells the
//!   window (`display_info`'s `repair`), and sweeps last runs' attempts (`sweep.rs`).

#[cfg(windows)]
mod app;
#[cfg(any(test, windows))]
mod fetch;
#[cfg(any(test, windows))]
mod flow;
#[cfg(any(test, windows))]
mod marker;
#[cfg(any(test, windows))]
mod plan;
#[cfg(any(test, windows))]
mod sweep;
#[cfg(test)]
mod tests;
#[cfg(windows)]
mod windows;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use clauseo_protocol::HostError;
use clauseo_settings::Locations;
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{Runtime, Webview};

use crate::bridge::Bridge;
/// What the window hears while the repair runs, on the command's channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
#[cfg_attr(
    not(any(test, windows)),
    expect(dead_code, reason = "the channel's type; nothing is sent off Windows")
)]
pub(crate) enum RepairStep {
    /// Downloading the installer and checking it.
    Downloading,
    /// Checked: the installer starts next, and Windows will ask for permission.
    Launching,
}

/// Set while the repair destroys the windows: `lib.rs` then holds the exit that closing the last
/// window asks for, so a failed start can bring the main window back.
pub(crate) static HANDING_OVER: AtomicBool = AtomicBool::new(false);

/// Whether `lib.rs` holds an exit request with `code`: one with no code (the last window
/// closing) while the repair destroys the windows ([`HANDING_OVER`]). The repair's own quit,
/// and every other, comes with a code and goes on.
pub(crate) fn holds_exit(code: Option<i32>) -> bool {
    code.is_none() && HANDING_OVER.load(Ordering::Acquire)
}

/// Set for a repair's lifetime, from its admission to its end (the app quitting, after a
/// launch): one repair at a time, and the bridge refuses `InstallUpdate` meanwhile.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// Whether a repair is running (see [`RUNNING`]).
pub(crate) fn running() -> bool {
    RUNNING.load(Ordering::Acquire)
}

/// Tests: marks a repair running, or not.
#[cfg(test)]
pub(crate) fn set_running(running: bool) {
    RUNNING.store(running, Ordering::Release);
}

/// What the window is told about the repair, through `display_info`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum RepairNote {
    /// This start follows the installer a repair ran: a fresh `repair-pending` marker for this
    /// version. While the admin copy is still missing the card says the installer didn't
    /// finish; with the copy there it says nothing (the installer's `--adopt` ran the set-up).
    Returned,
    /// The installer did not start, and the main window was built again.
    #[cfg_attr(
        not(windows),
        expect(dead_code, reason = "only Windows starts the installer")
    )]
    LaunchFailed,
}

/// The repair's state for this run: where attempts and the marker go, and the note.
pub(crate) struct RepairState {
    /// `updates/`, where attempts go (Windows only makes them).
    #[cfg(windows)]
    updates: std::path::PathBuf,
    /// The marker's path.
    #[cfg(windows)]
    marker: std::path::PathBuf,
    note: Mutex<Option<RepairNote>>,
    available: OnceLock<bool>,
}

impl RepairState {
    /// This start's state: the marker under `locations` read and deleted, `Returned` when it
    /// was fresh for `version`.
    pub(crate) fn at_start(locations: &Locations, version: &str) -> Self {
        let marker = locations.data_dir().join(MARKER);
        // Only Windows writes one.
        #[cfg(any(test, windows))]
        let returned = marker::consume(&marker, version, now_secs());
        #[cfg(not(any(test, windows)))]
        let returned = {
            let _ = version;
            false
        };
        #[cfg(not(windows))]
        let _ = marker;
        Self {
            #[cfg(windows)]
            updates: locations.updates_dir(),
            #[cfg(windows)]
            marker,
            note: Mutex::new(returned.then_some(RepairNote::Returned)),
            available: OnceLock::new(),
        }
    }

    /// The window's note.
    pub(crate) fn note(&self) -> Option<RepairNote> {
        *crate::bridge::lock(&self.note)
    }

    #[cfg(windows)]
    fn set_note(&self, note: RepairNote) {
        *crate::bridge::lock(&self.note) = Some(note);
    }

    /// Whether the repair may be offered: this build and install (read once), and the updater's
    /// `update` state when it is known (managed, scheduled or installing refuses).
    pub(crate) fn available<R: Runtime>(
        &self,
        app: &tauri::AppHandle<R>,
        update: Option<&clauseo_protocol::UpdateState>,
    ) -> bool {
        *self.available.get_or_init(|| eligible(app, None).is_ok()) && !update_refuses(update)
    }
}

/// Whether the updater's `update` state, when known, refuses a repair.
#[cfg(any(test, windows))]
fn update_refuses(update: Option<&clauseo_protocol::UpdateState>) -> bool {
    update.is_some_and(plan::update_refuses)
}

/// Off Windows nothing is available anyway.
#[cfg(not(any(test, windows)))]
fn update_refuses(_update: Option<&clauseo_protocol::UpdateState>) -> bool {
    false
}

/// Manages this start's [`RepairState`] in `app`, before the main window opens and asks for
/// it: the marker under `locations` read and deleted.
pub(crate) fn manage_at_start<R: Runtime, M: tauri::Manager<R>>(app: &M, locations: &Locations) {
    let version = app.package_info().version.to_string();
    app.manage(std::sync::Arc::new(RepairState::at_start(
        locations, &version,
    )));
}

/// The `repair-pending` marker's name in the data folder (`marker.rs`).
const MARKER: &str = "repair-pending.json";

/// Sweeps last runs' attempts under `updates` (`sweep.rs`), before the host's clean-up. Only
/// Windows makes them.
pub(crate) fn sweep_at_start(updates: &std::path::Path) {
    #[cfg(windows)]
    drop(sweep::sweep(updates));
    #[cfg(not(windows))]
    let _ = updates;
}

/// Now, in Unix seconds: the marker's one clock read (its write, and its age at the next start).
#[cfg(any(test, windows))]
#[expect(
    clippy::disallowed_methods,
    reason = "the repair marker's clock: written at the launch and read at the next start, here alone"
)]
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs())
}

/// The updater key the repair's installer must be signed with, for the build's release mode: the
/// config's `plugins.updater.pubkey` for a release, the test key for a test build (the `e2e`
/// build alone has it), none for a dry run.
#[cfg(any(test, windows))]
fn key_for(mode: crate::commands::ReleaseMode, config: &tauri::Config) -> Option<String> {
    use crate::commands::ReleaseMode;
    match mode {
        ReleaseMode::Release => super::plugin::Source::Config.pubkey(config),
        ReleaseMode::Test => super::choice::TEST_KEY.map(|key| key.trim().to_owned()),
        ReleaseMode::Dry => None,
    }
}

/// The plan for this app now, with the updater's `update` state when it was read.
#[cfg(windows)]
fn eligible<R: Runtime>(
    app: &tauri::AppHandle<R>,
    update: Option<&clauseo_protocol::UpdateState>,
) -> Result<plan::Plan, plan::NotEligible> {
    let mode = crate::commands::build_release_mode();
    let version = app.package_info().version.to_string();
    let pubkey = key_for(mode, app.config());
    plan::plan(plan::Inputs {
        os: std::env::consts::OS,
        target_arch: std::env::consts::ARCH,
        kind: super::install_kind::detect(),
        mode,
        version: &version,
        pubkey: pubkey.as_deref(),
        update,
    })
}

/// Off Windows nothing is eligible.
#[cfg(not(windows))]
fn eligible<R: Runtime>(
    _app: &tauri::AppHandle<R>,
    _update: Option<&clauseo_protocol::UpdateState>,
) -> Result<(), ()> {
    Err(())
}

/// `commands::repair_install`, once admitted (see the module's notes).
#[cfg(windows)]
pub(crate) async fn start<R: Runtime>(
    webview: &Webview<R>,
    bridge: &Bridge,
    steps: Channel<RepairStep>,
) -> Result<(), HostError> {
    use clauseo_protocol::{ErrorCode, HostQuery, QueryResult};
    use tauri::Manager as _;

    if RUNNING
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return Err(HostError::new(
            ErrorCode::CommandConflict,
            "the installer is already being downloaded",
        ));
    }
    let app = webview.app_handle().clone();
    // No answer from the host is no telling whether an update installs: refused.
    let update = match bridge.query(crate::MAIN_WINDOW, HostQuery::GetUpdate).await {
        Ok(QueryResult::Update(view)) => Some(view.state),
        _ => None,
    };
    let planned = match update {
        Some(state) => {
            let probe = app.clone();
            tauri::async_runtime::spawn_blocking(move || eligible(&probe, Some(&state)))
                .await
                .unwrap_or(Err(plan::NotEligible("the eligibility check stopped")))
        }
        None => Err(plan::NotEligible("the updater's state was not read")),
    };
    let result = match planned {
        Ok(plan) => run(app, plan, steps).await,
        Err(why) => {
            tracing::info!(target: super::install::LIFECYCLE, why = why.0, "repair: not eligible");
            Err(flow::Refusal::NotEligible)
        }
    };
    if result.is_err() {
        RUNNING.store(false, Ordering::Release);
    }
    result.map_err(flow::Refusal::error)
}

/// Runs `plan` over the app's seams, on a blocking thread: the hand-over sleeps and waits.
#[cfg(windows)]
async fn run<R: Runtime>(
    app: tauri::AppHandle<R>,
    plan: plan::Plan,
    steps: Channel<RepairStep>,
) -> Result<(), flow::Refusal> {
    use tauri::Manager as _;
    let Some(state) = app.try_state::<std::sync::Arc<RepairState>>() else {
        return Err(flow::Refusal::NotEligible);
    };
    let seams = app::AppSeams {
        updates: state.updates.clone(),
        marker: state.marker.clone(),
        version: plan.version.clone(),
        steps,
        app: app.clone(),
    };
    let client = client().map_err(|()| flow::Refusal::DownloadFailed)?;
    tauri::async_runtime::spawn_blocking(move || {
        tauri::async_runtime::block_on(flow::run(&seams, &client, &plan))
    })
    .await
    // A panicked repair thread: release builds abort on panic, so only a dev build gets here.
    .unwrap_or(Err(flow::Refusal::LaunchFailed))
}

/// Off Windows the repair never runs.
#[cfg(not(windows))]
pub(crate) fn start<R: Runtime>(
    _webview: &Webview<R>,
    _bridge: &Bridge,
    _steps: Channel<RepairStep>,
) -> std::future::Ready<Result<(), HostError>> {
    std::future::ready(Err(no_repair()))
}

/// `repair.not_eligible`, off Windows.
#[cfg(not(windows))]
fn no_repair() -> HostError {
    HostError::new(
        clauseo_protocol::ErrorCode::RepairNotEligible,
        "the installer can't be run again from here",
    )
}

/// The updater's HTTPS client (`plugin::client`), with ring as rustls' provider as the updater
/// and the counts' upload install it.
#[cfg(any(test, windows))]
fn client() -> Result<reqwest::Client, ()> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        drop(rustls::crypto::ring::default_provider().install_default());
    }
    super::plugin::client(reqwest::Client::builder())
        .build()
        .map_err(|_unbuilt| ())
}
