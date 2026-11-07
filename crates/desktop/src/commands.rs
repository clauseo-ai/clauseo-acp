//! The commands the webview may invoke, each `async` and granted one by one in
//! `capabilities/main.json` (and a subset to the settings window in `capabilities/settings.json`,
//! `docs/decisions/0028-settings-window.md`, which also records `open_settings`). Every
//! argument is checked in Rust before it reaches the host (`docs/decisions/0001`,
//! `docs/threat-model.md` "Webview"), and every call runs as `Principal::LocalDesktop`, which
//! the bridge supplies; the webview never names a principal.
//! `docs/decisions/0012-desktop-bridge.md` records the list and the stream's message format,
//! `docs/decisions/0019-app-stream-to-the-window.md` the tenth, `subscribe_app`, and the
//! allow-lists of `dispatch` and `query`, and `docs/decisions/0026-open-and-reveal.md` the three
//! that leave the app: `open_external`, `open_file` and `reveal` (`open.rs`).
//! `docs/decisions/0048-signin-on-the-wire.md` records `setup`, granted to `main` and the
//! settings window, each with its own allow-list (`setup.rs`).
//! `lib.rs` registers them with `tauri::generate_handler!`; tauri-specta reads them only in the
//! test that writes the TypeScript bindings (`commands/bindings.rs`).

use clauseo_protocol::{
    Ack, AgentChoiceId, CommandEnvelope, CommandOutcome, ErrorCode, FileRefId, HostError,
    HostQuery, PermissionRequestId, QueryResult, SetupCommand, StreamId, ThreadId, ViewPosition,
};
use clauseo_settings::{PlatformDirs as _, SystemDirs};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State, Webview};

use crate::bridge::Bridge;
use crate::chooser::{Chooser, Go, Listing, PickAnswer, Route, StartPoint};
use crate::counts::{CountsState, UiCount};
use crate::dialogs::LastPick;
use crate::frame::Frame;
use crate::open::{AppData, NativeLinkConfirm, NativeOpener, OpenGates, OpenedFile, RevealTarget};
use crate::picked::PickedFolder;

/// The command names, in the order `build.rs` and `capabilities/main.json` list them. A test
/// checks they, `lib.rs`'s handler and the bindings agree.
#[cfg(test)]
pub(crate) const COMMANDS: [&str; 22] = [
    "dispatch",
    "query",
    "open_stream",
    "subscribe",
    "unsubscribe",
    "subscribe_app",
    "ack",
    "answer_permission",
    "pick_folder",
    "pick_folder_or_choose",
    "chooser_starts",
    "chooser_list",
    "chooser_choose",
    "chooser_cancel",
    "chooser_shown",
    "display_info",
    "open_external",
    "open_file",
    "reveal",
    "open_settings",
    "count_ui",
    "setup",
    "quit_app",
];

/// The commands the `e2e` build adds (`e2e.rs`), as `build.rs`'s `E2E` and
/// `capabilities/e2e.json` list them. Not in the bindings: the page calls them with `invoke`.
#[cfg(test)]
pub(crate) const E2E_COMMANDS: [&str; 1] = ["perf_channel_bytes"];

/// The commands the `marks` build adds to e2e's (`marks.rs`), as `build.rs`'s `MARKS` and
/// `capabilities/marks.json` list them.
#[cfg(test)]
pub(crate) const MARKS_COMMANDS: [&str; 2] = ["marks_memory", "marks_finish"];

/// The OS the host runs on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub(crate) enum Platform {
    /// macOS.
    Mac,
    /// Windows.
    Windows,
    /// Linux.
    Linux,
}

impl Platform {
    /// The OS this build runs on.
    pub(crate) const CURRENT: Platform = if cfg!(target_os = "macos") {
        Platform::Mac
    } else if cfg!(windows) {
        Platform::Windows
    } else {
        Platform::Linux
    };
}

/// The release date release.yml builds a tagged release with (`YYYY-MM-DD`, UTC; `build.rs`
/// checks the form). Dev and CI builds have none.
const RELEASE_DATE: Option<&str> = option_env!("CLAUSEO_RELEASE_DATE");

/// What the webview may know about the machine and this build.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct DisplayInfo {
    /// The OS the host runs on.
    pub(crate) platform: Platform,
    /// The date of this release (`YYYY-MM-DD`, UTC), from its tag; `None` in dev and CI builds.
    pub(crate) release_date: Option<&'static str>,
    /// Whether this is a test build: an `e2e` build, which `test-v*` pre-releases ship with
    /// and anyone can install.
    pub(crate) test_build: bool,
    /// The build's release mode, from `CLAUSEO_RELEASE_MODE` at compile time. The UI uses it to
    /// choose the release page for the installer button: a release opens
    /// `…/releases/tag/v{version}`, a test build `…/releases/tag/test-v{version}`, and a dry build
    /// shows no button.
    pub(crate) release_mode: ReleaseMode,
}

/// The release mode of this build, as release.yml sets `CLAUSEO_RELEASE_MODE` for its cargo build.
const RELEASE_MODE_ENV: Option<&str> = option_env!("CLAUSEO_RELEASE_MODE");

/// What kind of build this is: a tagged release, a test build (`test-v*`), or a dry run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
#[serde(rename_all = "lowercase")]
pub(crate) enum ReleaseMode {
    /// A tagged release.
    Release,
    /// A test build, published as a `test-v*` pre-release.
    Test,
    /// A dry run: no release page.
    Dry,
}

/// The release mode from `CLAUSEO_RELEASE_MODE` as compiled in (`env`) and whether the `e2e`
/// feature is on. `release`, `test` and `dry` are their modes. Unset, an `e2e` build is a test
/// build and any other a release. Any other value is `Dry`: there is no page to point at.
pub(crate) fn release_mode(env: Option<&str>, e2e: bool) -> ReleaseMode {
    let unset = if e2e {
        ReleaseMode::Test
    } else {
        ReleaseMode::Release
    };
    match env {
        Some("release") => ReleaseMode::Release,
        Some("test") => ReleaseMode::Test,
        None => unset,
        // `dry`, and any other value: no page to point at
        Some(_) => ReleaseMode::Dry,
    }
}

/// Runs a command. Refuses `AnswerPermission` (that goes through `answer_permission`) and a
/// `CreateMatter` whose folder ref this run's `pick_folder` did not issue; from the settings
/// window, everything but `SetUiSettings`, `SetToolSettings`, `SetPrivacySettings`,
/// `SetSkillPack`, `RefreshLimits`, `SetUpdateMode`, `CheckForUpdate` and the connector
/// commands but `DismissCaseLawHint`; from `main`, `SetMode`, `SetConfigOption`,
/// `SetToolSettings`, `SetPrivacySettings`, `SetUpdateMode`, `SetSkillPack` and every
/// connector command but `DismissCaseLawHint` and `RefreshBalance` (`OpenPayLink` among them;
/// `src/bridge/admit.rs`).
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn dispatch<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Bridge>,
    command: CommandEnvelope,
) -> Result<CommandOutcome, HostError> {
    bridge.dispatch(webview.label(), command).await
}

/// Answers a query; from the settings window, `GetSettings` only.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn query<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Bridge>,
    query: HostQuery,
) -> Result<QueryResult, HostError> {
    bridge.query(webview.label(), query).await
}

/// Registers the calling window's one channel. A second call (a page reload) replaces it and
/// drops every subscription of the old one.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn open_stream<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Bridge>,
    channel: Channel<Frame>,
) -> Result<(), HostError> {
    bridge.open_stream(webview.label(), channel).await
}

/// Subscribes the window's channel to `stream`: the tail after `after` or one `Reset`, then
/// live patches. Subscribing a stream already subscribed restarts it from `after`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn subscribe<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Bridge>,
    stream: StreamId,
    after: Option<ViewPosition>,
) -> Result<(), HostError> {
    bridge.subscribe(webview.label(), stream, after).await
}

/// Ends the window's subscription to `stream`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn unsubscribe<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Bridge>,
    stream: StreamId,
) -> Result<(), HostError> {
    bridge.unsubscribe(webview.label(), stream)
}

/// Subscribes the window's channel to the app-wide events, those of no stream (today
/// `settings_changed`). It starts with nothing: subscribe, then read the current state with
/// `query`. Subscribing again restarts it; a new `open_stream` drops it.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn subscribe_app<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Bridge>,
) -> Result<(), HostError> {
    bridge.subscribe_app(webview.label()).await
}

/// Records that the window has received and processed `messages` channel messages since
/// `open_stream`, and applied each stream in `positions` (maybe none) up to its cursor. Refused
/// whole when `messages` is more than the window was sent or a stream is named twice.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn ack<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Bridge>,
    positions: Vec<Ack>,
    messages: u32,
) -> Result<(), HostError> {
    bridge.ack(webview.label(), positions, messages).await
}

/// Answers a permission request with an option it offered. An `allow_always` option first asks
/// the lawyer in a native dialog; declined there, the call fails with `transport.unauthorized`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn answer_permission<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    thread: ThreadId,
    request: PermissionRequestId,
    option: AgentChoiceId,
) -> Result<(), HostError> {
    let confirm = crate::dialogs::NativeConfirm::new(app);
    bridge
        .answer_permission(&confirm, thread, request, option)
        .await
}

/// Opens the native folder dialog and registers the folder picked: its ref, and its display
/// relative to the home folder (`picked.rs`). `None` when cancelled.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn pick_folder<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    last: State<'_, LastPick>,
) -> Result<Option<PickedFolder>, HostError> {
    native_pick(app, &bridge, &last).await
}

/// [`pick_folder`]'s native dialog.
async fn native_pick<R: Runtime>(
    app: AppHandle<R>,
    bridge: &Bridge,
    last: &LastPick,
) -> Result<Option<PickedFolder>, HostError> {
    let home = SystemDirs.home();
    let start = crate::dialogs::start_folder(last.get().as_deref(), home.as_deref());
    // Windows only: its dialog would otherwise open on Documents, which may sit on a share that
    // is down. Elsewhere the OS's own remembered folder stays.
    let start = if cfg!(windows) { start } else { None };
    let picker = crate::dialogs::NativePicker::new(app, start, last);
    bridge.pick_folder(&picker, home.as_deref()).await
}

/// [`pick_folder`], unless Windows' dialog would stall on a known folder whose share is down
/// (`chooser/stall.rs`, found out as the app starts, so at once by now, at most 1.5 s after the
/// start; `CLAUSEO_FOLDER_PICKER=app|native` decides instead): then it opens an in-app chooser
/// session (closing any other) and answers `choose` at once, without waiting on any read. At
/// most 20 sessions a run and 3 a minute; past that the native dialog opens. Never `choose` off
/// Windows.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn pick_folder_or_choose<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    last: State<'_, LastPick>,
    chooser: State<'_, Chooser>,
    app_data: State<'_, AppData>,
) -> Result<PickAnswer, HostError> {
    let clicked = crate::open::now();
    crate::chooser::trace::step("click received", clicked, "");
    // The e2e build's folder comes first, as `NativePicker::pick` gives it: no check, no session.
    #[cfg(feature = "e2e")]
    let fixed = crate::e2e::matter_dir();
    #[cfg(not(feature = "e2e"))]
    let fixed: Option<std::path::PathBuf> = None;
    let setting = std::env::var(crate::chooser::PICKER_VAR).ok();
    let stalls = chooser.inner().clone();
    let has_fixed = fixed.is_some();
    let route = tauri::async_runtime::spawn_blocking(move || {
        // Past the session caps the lawyer sees the native dialog; no check runs then.
        let may_open = cfg!(windows) && stalls.may_open(crate::open::now());
        crate::chooser::route(has_fixed, may_open, setting.as_deref(), || {
            stalls.would_stall()
        })
    })
    .await
    .unwrap_or(Route::Native);
    if let (Route::Fixed, Some(folder)) = (route, fixed) {
        return crate::chooser::pick_fixed(&bridge, folder, SystemDirs.home().as_deref()).await;
    }
    crate::chooser::trace::step("route decided", clicked, &format!("{route:?}"));
    if route == Route::Chooser {
        let data = app_data.0.clone();
        // Home and the refused folders ask Windows for known folders, which may wait on a share:
        // read on the session's own thread, so the answer goes back at once.
        let asked = move || {
            let mut refused = clauseo_settings::refused_folders(&SystemDirs);
            refused.push(data);
            (SystemDirs.home(), refused)
        };
        let now = crate::open::now();
        if let Some(start) = chooser.open_asking(asked, last.get().as_deref(), now) {
            crate::chooser::trace::step("session opened", clicked, "");
            return Ok(PickAnswer::Choose { start });
        }
    }
    Ok(match native_pick(app, &bridge, &last).await? {
        Some(folder) => PickAnswer::Picked { folder },
        None => PickAnswer::Cancelled,
    })
}

/// Where the chooser session `chooser` may start: its drives, home, Desktop and last place,
/// once its one read of the drives landed (at most 1.5 s after it opened). `command.invalid`
/// for an unknown or closed session.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_starts(
    state: State<'_, Chooser>,
    chooser: u32,
) -> Result<Vec<StartPoint>, HostError> {
    let state = state.inner().clone();
    let asked = crate::open::now();
    let starts =
        tauri::async_runtime::spawn_blocking(move || state.starts(chooser, crate::open::now()))
            .await
            .map_err(|error| HostError::new(ErrorCode::Internal, format!("no starts: {error}")))?;
    crate::chooser::trace::step("starts answered", asked, "");
    starts
}

/// The page's timing of a chooser it showed, for the log alone (`chooser/trace.rs`): the
/// milliseconds from the click to the desktop's answer, to the chooser's first render and to its
/// first paint, as the page measured them. Always answers ok; logs no path, and only the first
/// [`CHOOSER_SHOWN_CAP`] calls of a run.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_shown(
    answer_ms: u32,
    render_ms: u32,
    paint_ms: u32,
) -> Result<(), HostError> {
    if CHOOSER_SHOWN_LOGGED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) < CHOOSER_SHOWN_CAP {
        tracing::info!(
            at_ms = crate::chooser::trace::at_ms(),
            answer_ms,
            render_ms,
            paint_ms,
            "folder chooser: the page showed it"
        );
    }
    Ok(())
}

/// How many `chooser_shown` lines one run logs, so script in the page can't flood host.log.
const CHOOSER_SHOWN_CAP: u32 = 20;

/// The `chooser_shown` calls so far this run, logged or not.
static CHOOSER_SHOWN_LOGGED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Goes where `go` says in the chooser session `chooser` and lists the folder there: by a
/// start point's key, an index into the last listing, up (to a drive or typed share root), a
/// typed path, or again. At most 2 s: `no_answer` past it. `command.invalid` for an unknown or
/// closed session, an index or key it did not give, and a typed path it refuses;
/// `transport.unauthorized` past 300 listings.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_list(
    state: State<'_, Chooser>,
    chooser: u32,
    go: Go,
) -> Result<Listing, HostError> {
    let state = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || state.list(chooser, go, crate::open::now()))
        .await
        .map_err(|error| HostError::new(ErrorCode::Internal, format!("no listing: {error}")))?
}

/// Chooses the folder the chooser session shows, after a native confirm with the full path and
/// Cancel focused; confirmed, it is registered as `pick_folder` registers a pick and the session
/// closes. `None` when the lawyer declines or the folder has gone; the session stays open.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_choose<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    last: State<'_, LastPick>,
    state: State<'_, Chooser>,
    chooser: u32,
) -> Result<Option<PickedFolder>, HostError> {
    let confirm = crate::chooser::NativeFolderConfirm::new(app);
    let home = SystemDirs.home();
    crate::chooser::choose_folder(&state, &bridge, &confirm, &last, home.as_deref(), chooser).await
}

/// Closes the chooser session; `command.invalid` when it is not open.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn chooser_cancel(
    state: State<'_, Chooser>,
    chooser: u32,
) -> Result<(), HostError> {
    state.close(chooser)
}

/// The OS the host runs on, this build's release date, whether it is a test build, and its
/// release mode.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn display_info<R: Runtime>(_app: AppHandle<R>) -> Result<DisplayInfo, HostError> {
    Ok(DisplayInfo {
        platform: Platform::CURRENT,
        release_date: RELEASE_DATE,
        test_build: cfg!(feature = "e2e"),
        release_mode: release_mode(RELEASE_MODE_ENV, cfg!(feature = "e2e")),
    })
}

/// Opens an https, http or mailto `url` in the default browser or mail app, after a native
/// confirm that names where it goes, in the app's language. Any other link is
/// `command.invalid`; declined, nothing opens and the call fails with `transport.unauthorized`.
/// One confirm at a time: `command.conflict` while one is open, and for a second after a decline.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn open_external<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    gates: State<'_, OpenGates>,
    url: String,
) -> Result<(), HostError> {
    // The link is checked before anything else, the settings read included.
    let link = crate::open::check_link(&url)?;
    let language = bridge.language().await;
    let confirm = NativeLinkConfirm::new(app.clone());
    let opener = NativeOpener::new(app);
    crate::open::open_external(
        &confirm,
        &opener,
        &gates.links,
        &crate::open::now,
        language,
        &link,
    )
    .await
}

/// Opens the document `file` names with its default app. Refused with `command.invalid` when
/// the host did not issue the ref, when the file is not inside its matter's folder, and when
/// it is not a document type (`open::DOCUMENT_TYPES`). One at a time and at most one a second:
/// `command.conflict` otherwise. A Word document whose path is too long for Word opens in
/// Protected View (`protected_view`), or is refused with `matter.document_path_too_long`.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn open_file<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    gates: State<'_, OpenGates>,
    file: FileRefId,
) -> Result<OpenedFile, HostError> {
    let opener = std::sync::Arc::new(NativeOpener::new(app));
    crate::open::open_file(
        bridge.host().await?,
        &opener,
        &gates.files,
        &crate::open::now,
        crate::open::FOLDER_WAIT,
        file,
    )
    .await
}

/// Shows a matter's folder, or the app data folder, in Finder or File Explorer.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn reveal<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    app_data: State<'_, AppData>,
    target: RevealTarget,
) -> Result<(), HostError> {
    crate::open::reveal(
        bridge.host().await?,
        &std::sync::Arc::new(NativeOpener::new(app)),
        &app_data.0,
        crate::open::FOLDER_WAIT,
        target,
    )
    .await
}

/// Opens the settings window, or brings it forward when it is open (`settings_window.rs`), at
/// `section` when one is named (`"connectors"`: the room's case-law hint and banner;
/// `"protection"`: UC's first-run button, R17). An
/// unknown section is refused as the arguments decode. Granted to `main` only.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn open_settings<R: Runtime>(
    app: AppHandle<R>,
    section: Option<crate::settings_window::SettingsSection>,
) -> Result<(), HostError> {
    crate::settings_window::open_or_focus(&app, section).map_err(|error| {
        HostError::new(
            ErrorCode::Internal,
            format!("the settings window did not open: {error}"),
        )
    })
}

/// Quits the app the way Cmd+Q does: `app.exit` raises `RunEvent::ExitRequested` (`lib.rs`), so
/// the agents' installs and sign-ins stop and the host stops before the app ends. Granted to
/// `main` and the settings window, whose Get the installer button asks for it once the release
/// page opened (`ui/app/src/room/protection/installer.ts`).
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn quit_app<R: Runtime>(app: AppHandle<R>) -> Result<(), HostError> {
    app.exit(0);
    Ok(())
}

/// Counts one of the events only the webview sees (`counts/ui.rs`, 0036). Granted to `main`
/// alone, by its own capability (`capabilities/counts.json`). The counts open before the host
/// starts, so it waits for the host's start, whatever its outcome; it never fails.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn count_ui(
    bridge: State<'_, Bridge>,
    counts: State<'_, CountsState>,
    event: UiCount,
) -> Result<(), HostError> {
    let _started = bridge.host().await;
    counts.count(event);
    Ok(())
}

/// Runs an install or sign-in command (0048): refused with `transport.unauthorized`, before
/// anything runs, when the window may not send it (`main` the first-run and sign-in actions and
/// a first key, the settings window every action). A `StartSignIn` for a sign-in already
/// running answers `Done` and joins it. What changes comes on the window's channel as `setup`
/// messages, after `subscribe_app`. Never waits for the host.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn setup<R: Runtime>(
    webview: Webview<R>,
    bridge: State<'_, Bridge>,
    command: SetupCommand,
) -> Result<CommandOutcome, HostError> {
    bridge.setup(webview.label(), command).await
}

#[cfg(test)]
mod bindings;
#[cfg(test)]
mod tests;
