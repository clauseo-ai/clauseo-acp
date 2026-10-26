//! The host's answers to the agent's requests, and `session/cancel`, which answers the pending
//! permission requests of its session as `cancelled`.

use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::{Agent, ConnectionTo, Responder};
use clauseo_protocol::{AgentChoiceId, AgentSessionId, ElicitationResponse};
use serde_json::Value;

use crate::Error;
use crate::driver::known;
use crate::event::{Origin, PendingKey};
use crate::state::Shared;

/// Answers a permission request with an option the agent offered; any other option leaves it
/// pending.
pub(crate) fn permission(
    shared: &Shared,
    key: PendingKey,
    option_id: &AgentChoiceId,
) -> Result<(), Error> {
    let mut state = shared.state();
    let Some(pending) = state.permissions.get(&key) else {
        return Err(Error::PermissionNotPending);
    };
    if !pending
        .options
        .iter()
        .any(|offered| offered == option_id.as_str())
    {
        return Err(Error::UnknownOption);
    }
    let pending = state
        .permissions
        .remove(&key)
        .ok_or(Error::PermissionNotPending)?;
    let outcome = acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(
        option_id.as_str().to_owned(),
    ));
    respond(
        pending.responder,
        &acp::RequestPermissionResponse::new(outcome),
    )
}

/// Answers an elicitation. A URL elicitation's accept carries no `content`; a form's carries the
/// values as JSON.
pub(crate) fn elicitation(
    shared: &Shared,
    key: PendingKey,
    response: ElicitationResponse,
) -> Result<(), Error> {
    let pending = shared.state().elicitations.remove(&key);
    let pending = pending.ok_or(Error::ElicitationNotPending)?;
    let body = match response {
        ElicitationResponse::Accept { .. } if pending.url => {
            serde_json::json!({"action": "accept"})
        }
        ElicitationResponse::Accept { values } => {
            let content = serde_json::to_value(&values)
                .map_err(|error| Error::Protocol(format!("elicitation values: {error}")))?;
            serde_json::json!({"action": "accept", "content": content})
        }
        ElicitationResponse::Decline => serde_json::json!({"action": "decline"}),
        ElicitationResponse::Cancel => serde_json::json!({"action": "cancel"}),
    };
    pending
        .responder
        .respond(body)
        .map_err(|error| closed(&error))
}

/// Sends `session/cancel`, moves the session's turn to `Late`, and answers its pending
/// permission requests `cancelled`, returning their keys.
pub(crate) fn cancel(
    cx: &ConnectionTo<Agent>,
    shared: &Shared,
    session: &AgentSessionId,
) -> Result<Vec<PendingKey>, Error> {
    if !known(shared, session) {
        return Err(Error::UnknownSession);
    }
    cx.send_notification(acp::CancelNotification::new(session.as_str().to_owned()))
        .map_err(|error| closed(&error))?;
    let mut state = shared.state();
    if let Some(entry) = state.sessions.get_mut(session.as_str())
        && let Origin::Turn(turn) = entry.origin
    {
        entry.origin = Origin::Late(turn);
    }
    let keys: Vec<PendingKey> = state
        .permissions
        .iter()
        .filter(|(_, pending)| pending.session == session.as_str())
        .map(|(key, _)| *key)
        .collect();
    let cancelled = acp::RequestPermissionResponse::new(acp::RequestPermissionOutcome::Cancelled);
    for key in &keys {
        if let Some(pending) = state.permissions.remove(key) {
            respond(pending.responder, &cancelled)?;
        }
    }
    Ok(keys)
}

fn respond<T: serde::Serialize>(responder: Responder<Value>, body: &T) -> Result<(), Error> {
    let body = serde_json::to_value(body).map_err(|error| Error::Protocol(error.to_string()))?;
    responder.respond(body).map_err(|error| closed(&error))
}

/// The SDK refuses to send only once the connection is closed.
fn closed(_error: &agent_client_protocol::Error) -> Error {
    Error::Closed
}
