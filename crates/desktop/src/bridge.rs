//! The bridge between the webview's commands and the session host. It holds the
//! [`HostHandle`], each window's channel and stream state (`window.rs`, driven by `pump.rs`),
//! and the folder refs this run's `pick_folder` issued. Every host call runs as
//! `Principal::LocalDesktop`; the webview never names a principal.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use clauseo_host::HostHandle;
use clauseo_protocol::{
    AgentChoiceId, CommandEnvelope, CommandId, CommandOutcome, Cursor, ErrorCode, FolderRefId,
    HostCommand, HostError, HostQuery, MAX_PAGE_ENTRIES, MatterSource, PageLimit, PermissionEntry,
    PermissionKind, PermissionRequestId, PermissionState, Principal, QueryResult, StreamId,
    ThreadId, ViewEntry, ViewEpoch, ViewPosition,
};
use tauri::ipc::Channel;

use crate::dialogs::{Confirm, Picker};
use crate::frame::Frame;

mod pump;
mod window;

use pump::Window;
use window::AckError;

/// The principal of every call: the desktop app on this computer.
const PRINCIPAL: Principal = Principal::LocalDesktop;

/// What the commands share: the host, the windows and the folder refs issued.
pub(crate) struct Bridge {
    host: HostHandle,
    /// Each window's stream, by webview label.
    windows: Mutex<HashMap<String, Arc<Mutex<Window>>>>,
    /// The folder refs `pick_folder` issued in this run; `CreateMatter` takes no other.
    issued: Mutex<HashSet<FolderRefId>>,
}

/// Locks `mutex`, taking the data of one a panicking holder left: every critical section here
/// leaves the state whole between statements.
pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A `command.invalid` failure with `message`.
fn invalid(message: impl Into<String>) -> HostError {
    HostError::new(ErrorCode::CommandInvalid, message)
}

impl Bridge {
    /// A bridge to `host` with no window yet.
    pub(crate) fn new(host: HostHandle) -> Self {
        Self {
            host,
            windows: Mutex::new(HashMap::new()),
            issued: Mutex::new(HashSet::new()),
        }
    }

    /// The host handle, for shutdown.
    pub(crate) fn host(&self) -> &HostHandle {
        &self.host
    }

    /// Checks `command`'s shape, refuses what goes another way, and runs it.
    pub(crate) async fn dispatch(
        &self,
        command: CommandEnvelope,
    ) -> Result<CommandOutcome, HostError> {
        let checked = command.validate().map_err(HostError::invalid_command)?;
        match checked.command() {
            HostCommand::AnswerPermission { .. } => {
                return Err(invalid(
                    "a permission is answered with answer_permission, not dispatch",
                ));
            }
            HostCommand::CreateMatter { source, .. } => {
                let (MatterSource::NewFolder { folder } | MatterSource::ExistingFolder { folder }) =
                    source;
                if !lock(&self.issued).contains(folder) {
                    return Err(invalid(format!(
                        "folder ref {folder} was not picked in this run"
                    )));
                }
            }
            _ => {}
        }
        self.host.dispatch(checked.into_envelope()).await
    }

    /// Answers `query`.
    pub(crate) async fn query(&self, query: HostQuery) -> Result<QueryResult, HostError> {
        self.host.query(PRINCIPAL, query).await
    }

    /// The window `label`'s stream, or `command.invalid` before its `open_stream`.
    fn window(&self, label: &str) -> Result<Arc<Mutex<Window>>, HostError> {
        lock(&self.windows)
            .get(label)
            .map(Arc::clone)
            .ok_or_else(|| invalid(format!("window {label} has not opened its stream")))
    }

    /// Makes `channel` the window's one channel. A window that had one (a page reload) drops
    /// it and every subscription it had.
    pub(crate) fn open_stream(&self, label: &str, channel: Channel<Frame>) {
        let window = Window::open(self.host.clone(), channel);
        let old = lock(&self.windows).insert(label.to_owned(), window);
        drop(old);
    }

    /// Follows `stream` on the window's channel from `after`.
    pub(crate) async fn subscribe(
        &self,
        label: &str,
        stream: StreamId,
        after: Option<ViewPosition>,
    ) -> Result<(), HostError> {
        let window = self.window(label)?;
        let after = after.map(|position| (position.epoch, position.cursor));
        let restart = pump::follow(&window, stream, after);
        pump::start(&self.host, &window, restart).await
    }

    /// Stops following `stream` on the window's channel.
    pub(crate) fn unsubscribe(&self, label: &str, stream: StreamId) -> Result<(), HostError> {
        let window = self.window(label)?;
        pump::unfollow(&window, stream);
        Ok(())
    }

    /// Records the window's ack and passes the position on to the host.
    pub(crate) async fn ack(
        &self,
        label: &str,
        stream: StreamId,
        epoch: ViewEpoch,
        cursor: Cursor,
        frames: u32,
    ) -> Result<(), HostError> {
        let window = self.window(label)?;
        match pump::ack(&window, stream, (epoch, cursor), frames) {
            Ok(()) => {}
            Err(AckError::TooManyFrames) => {
                return Err(invalid(format!(
                    "ack of {frames} frames is more than the window was sent"
                )));
            }
        }
        self.host.ack(PRINCIPAL, stream, epoch, cursor).await
    }

    /// Answers a permission request with `option`, which it must have offered. An
    /// `allow_always` option first goes past `confirm`; declined there, nothing is answered and
    /// the call fails with `transport.unauthorized`.
    pub(crate) async fn answer_permission(
        &self,
        confirm: &dyn Confirm,
        thread: ThreadId,
        request: PermissionRequestId,
        option: AgentChoiceId,
    ) -> Result<(), HostError> {
        let entry = self.pending_permission(thread, request).await?;
        let offered = entry
            .options
            .iter()
            .find(|offered| offered.option_id.as_str() == option.as_str())
            .ok_or_else(|| {
                HostError::new(
                    ErrorCode::PermissionUnknownOption,
                    format!("permission request {request} did not offer option {option}"),
                )
            })?;
        if offered.kind == PermissionKind::AllowAlways && !confirm.confirm().await {
            return Err(HostError::new(
                ErrorCode::TransportUnauthorized,
                "the lawyer declined to always allow",
            ));
        }
        let command_id =
            CommandId::new(uuid::Uuid::now_v7()).map_err(HostError::invalid_command)?;
        let answer = CommandEnvelope {
            command_id,
            command: HostCommand::AnswerPermission {
                thread,
                request,
                option_id: option,
            },
        };
        let checked = answer.validate().map_err(HostError::invalid_command)?;
        self.host.dispatch(checked.into_envelope()).await?;
        Ok(())
    }

    /// The permission entry `request` of `thread`'s view, when it is pending. Pages back from
    /// the newest entry: a pending request is among the last.
    async fn pending_permission(
        &self,
        thread: ThreadId,
        request: PermissionRequestId,
    ) -> Result<PermissionEntry, HostError> {
        let not_pending = || {
            HostError::new(
                ErrorCode::PermissionNotPending,
                format!("permission request {request} is not pending"),
            )
        };
        let limit = PageLimit::new(MAX_PAGE_ENTRIES).map_err(HostError::invalid_command)?;
        let mut before_order = None;
        loop {
            let page = HostQuery::PageEntries {
                stream: StreamId::Thread(thread),
                before_order,
                limit,
            };
            let QueryResult::Entries(entries) = self.host.query(PRINCIPAL, page).await? else {
                return Err(HostError::new(
                    ErrorCode::Internal,
                    "PageEntries answered with another kind of result",
                ));
            };
            let found = entries.iter().rev().find_map(|entry| match entry {
                ViewEntry::Permission(permission) if permission.request == request => {
                    Some(permission)
                }
                _ => None,
            });
            if let Some(permission) = found {
                return match permission.state {
                    PermissionState::Pending => Ok(permission.clone()),
                    _ => Err(not_pending()),
                };
            }
            let full = u32::try_from(entries.len()).is_ok_and(|len| len >= limit.get());
            match entries.first() {
                Some(oldest) if full => before_order = Some(oldest.order()),
                _ => return Err(not_pending()),
            }
        }
    }

    /// Asks `picker` for a folder and registers it; `None` when the lawyer cancelled. Whether
    /// the folder may be a matter is the host's rule: `CreateMatter` refuses a refused folder
    /// with `matter.folder_refused`.
    pub(crate) async fn pick_folder(
        &self,
        picker: &dyn Picker,
    ) -> Result<Option<FolderRefId>, HostError> {
        let Some(path) = picker.pick().await else {
            return Ok(None);
        };
        let folder = self.host.register_folder(path).await?;
        lock(&self.issued).insert(folder);
        Ok(Some(folder))
    }
}

#[cfg(test)]
mod tests;
