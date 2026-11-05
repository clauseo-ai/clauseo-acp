//! Answering a permission request (`Bridge::answer_permission`): the option must be one the
//! request offered, the options that change the session's mode are refused (`MODE_OPTIONS`),
//! and an `allow_always` asks the lawyer first (`needs_confirm`).

use clauseo_protocol::{
    AgentChoiceId, ErrorCode, HostCommand, HostError, HostQuery, MAX_PAGE_ENTRIES, PageLimit,
    PermissionEntry, PermissionKind, PermissionRequestId, PermissionState, QueryResult, StreamId,
    ThreadId, ViewEntry,
};

use super::{Bridge, MODE_OPTIONS, PRINCIPAL, invalid};
use crate::dialogs::Confirm;

/// Whether answering with an option of `kind` first needs the lawyer's native confirm. Fails
/// closed: only the once kinds and `reject_always` skip it, `allow_always` needs it, and a
/// kind this version does not know is refused with `command.invalid`.
pub(super) fn needs_confirm(kind: PermissionKind) -> Result<bool, HostError> {
    match kind {
        PermissionKind::AllowOnce | PermissionKind::RejectOnce | PermissionKind::RejectAlways => {
            Ok(false)
        }
        PermissionKind::AllowAlways => Ok(true),
        PermissionKind::Unknown => Err(invalid(
            "an option of a kind this version does not know cannot be answered",
        )),
    }
}

/// Answers a permission request with `option`, which it must have offered. An
/// `allow_always` option first goes past `confirm`; declined there, nothing is answered and
/// the call fails with `transport.unauthorized`. One of [`MODE_OPTIONS`], or an option of a
/// kind this version does not know, is refused with `command.invalid`.
pub(super) async fn answer(
    bridge: &Bridge,
    confirm: &dyn Confirm,
    thread: ThreadId,
    request: PermissionRequestId,
    option: AgentChoiceId,
) -> Result<(), HostError> {
    if MODE_OPTIONS.contains(&option.as_str()) {
        return Err(invalid(format!(
            "option {option} changes the session's mode, which the window cannot do yet"
        )));
    }
    let entry = bridge.pending_permission(thread, request).await?;
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
    if needs_confirm(offered.kind)? && !confirm.confirm().await {
        return Err(HostError::new(
            ErrorCode::TransportUnauthorized,
            "the lawyer declined to always allow",
        ));
    }
    let answer = HostCommand::AnswerPermission {
        thread,
        request,
        option_id: option,
    };
    bridge.run(answer).await?;
    Ok(())
}

/// The permission entry `request` of `thread`'s view, when it is pending. Pages back from
/// the newest entry: a pending request is among the last.
pub(super) async fn pending(
    bridge: &Bridge,
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
        let QueryResult::Entries(entries) = bridge.host().await?.query(PRINCIPAL, page).await?
        else {
            return Err(HostError::new(
                ErrorCode::Internal,
                "PageEntries answered with another kind of result",
            ));
        };
        let found = entries.iter().rev().find_map(|entry| match entry {
            ViewEntry::Permission(permission) if permission.request == request => Some(permission),
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
