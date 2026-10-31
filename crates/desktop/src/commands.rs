//! The commands the webview may invoke, each `async` and granted one by one in
//! `capabilities/main.json`. Every argument is checked in Rust before it reaches the host
//! (`docs/decisions/0001`, `docs/threat-model.md` "Webview"), and every call runs as
//! `Principal::LocalDesktop`, which the bridge supplies; the webview never names a principal.
//! `docs/decisions/0012-desktop-bridge.md` records the list and the stream's message format.
//! `lib.rs` registers them with `tauri::generate_handler!`; tauri-specta reads them only in the
//! test that writes the TypeScript bindings (`commands/bindings.rs`).

use clauseo_protocol::{
    Ack, AgentChoiceId, CommandEnvelope, CommandOutcome, FolderRefId, HostError, HostQuery,
    PermissionRequestId, QueryResult, StreamId, ThreadId, ViewPosition,
};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Runtime, State, Webview};

use crate::bridge::Bridge;
use crate::frame::Frame;

/// The command names, in the order `build.rs` and `capabilities/main.json` list them. A test
/// checks they, `lib.rs`'s handler and the bindings agree.
#[cfg(test)]
pub(crate) const COMMANDS: [&str; 9] = [
    "dispatch",
    "query",
    "open_stream",
    "subscribe",
    "unsubscribe",
    "ack",
    "answer_permission",
    "pick_folder",
    "display_info",
];

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

/// What the webview may know about the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(test, derive(specta::Type))]
pub(crate) struct DisplayInfo {
    /// The OS the host runs on.
    pub(crate) platform: Platform,
}

/// Runs a command. Refuses `AnswerPermission` (that goes through `answer_permission`) and a
/// `CreateMatter` whose folder ref this run's `pick_folder` did not issue.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn dispatch<R: Runtime>(
    _app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    command: CommandEnvelope,
) -> Result<CommandOutcome, HostError> {
    bridge.dispatch(command).await
}

/// Answers a query.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn query<R: Runtime>(
    _app: AppHandle<R>,
    bridge: State<'_, Bridge>,
    query: HostQuery,
) -> Result<QueryResult, HostError> {
    bridge.query(query).await
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
    bridge.open_stream(webview.label(), channel);
    Ok(())
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

/// Opens the native folder dialog and registers the folder picked; `None` when cancelled.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn pick_folder<R: Runtime>(
    app: AppHandle<R>,
    bridge: State<'_, Bridge>,
) -> Result<Option<FolderRefId>, HostError> {
    let picker = crate::dialogs::NativePicker::new(app);
    bridge.pick_folder(&picker).await
}

/// The OS the host runs on.
#[tauri::command]
#[cfg_attr(test, specta::specta)]
pub(crate) async fn display_info<R: Runtime>(_app: AppHandle<R>) -> Result<DisplayInfo, HostError> {
    Ok(DisplayInfo {
        platform: Platform::CURRENT,
    })
}

#[cfg(test)]
mod bindings;
#[cfg(test)]
mod tests;
