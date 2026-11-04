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
//! `lib.rs` registers them with `tauri::generate_handler!`; tauri-specta reads them only in the
//! test that writes the TypeScript bindings (`commands/bindings.rs`).

use clauseo_protocol::{
    Ack, AgentChoiceId, CommandEnvelope, CommandOutcome, FileRefId, HostError, HostQuery,
    PermissionRequestId, QueryResult, StreamId, ThreadId, ViewPosition,
};
use clauseo_settings::{PlatformDirs as _, SystemDirs};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State, Webview};

use crate::bridge::Bridge;
use crate::counts::{CountsState, UiCount};
use crate::frame::Frame;
use crate::open::{AppData, NativeLinkConfirm, NativeOpener, OpenGates, RevealTarget};
use crate::picked::PickedFolder;

/// The command names, in the order `build.rs` and `capabilities/main.json` list them. A test
/// checks they, `lib.rs`'s handler and the bindings agree.
#[cfg(test)]
pub(crate) const COMMANDS: [&str; 15] = [
    "dispatch",
    "query",
    "open_stream",
    "subscribe",
    "unsubscribe",
    "subscribe_app",
    "ack",
    "answer_permission",
    "pick_folder",
    "display_info",
    "open_external",
    "open_file",
    "reveal",
    "open_settings",
    "count_ui",
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
}

/// Runs a command. Refuses `AnswerPermission` (that goes through `answer_permission`) and a
/// `CreateMatter` whose folder ref this run's `pick_folder` did not issue; from the settings
/// window, everything but `SetUiSettings` and `SetToolSettings`.
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
) -> Result<Option<PickedFolder>, HostError> {
    let picker = crate::dialogs::NativePicker::new(app);
    let home = SystemDirs.home();
    bridge.pick_folder(&picker, home.as_deref()).await
}

/// The OS the host runs on, and this build's release date.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn display_info<R: Runtime>(_app: AppHandle<R>) -> Result<DisplayInfo, HostError> {
    Ok(DisplayInfo {
        platform: Platform::CURRENT,
        release_date: RELEASE_DATE,
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
/// `command.conflict` otherwise.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn open_file<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    gates: State<'_, OpenGates>,
    file: FileRefId,
) -> Result<(), HostError> {
    let opener = NativeOpener::new(app);
    crate::open::open_file(
        bridge.host().await?,
        &opener,
        &gates.files,
        &crate::open::now,
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
        &NativeOpener::new(app),
        &app_data.0,
        target,
    )
    .await
}

/// Opens the settings window, or brings it forward when it is open (`settings_window.rs`).
/// Granted to `main` only.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn open_settings<R: Runtime>(app: AppHandle<R>) -> Result<(), HostError> {
    crate::settings_window::open_or_focus(&app).map_err(|error| {
        HostError::new(
            clauseo_protocol::ErrorCode::Internal,
            format!("the settings window did not open: {error}"),
        )
    })
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

#[cfg(test)]
mod bindings;
#[cfg(test)]
mod tests;
