//! The one SDK handler: every message the agent sends passes through [`Inbound`]. It only
//! forwards: it maps, records what an answer needs, sends events and returns. While the event
//! channel is full it waits, which holds the SDK's dispatch loop and stops the line reader.
//!
//! It takes messages untyped, because SDK 3.3.0 drops a typed notification that fails to decode
//! with only a log line: an unknown `sessionUpdate` would vanish without
//! [`ProtocolWarning::UnknownUpdate`]. Responses pass through to the SDK's router.

use std::sync::Arc;

use agent_client_protocol::schema::v1 as acp;
use agent_client_protocol::{
    Agent, ConnectionTo, Dispatch, HandleDispatchFrom, Handled, Responder, UntypedMessage,
};
use clauseo_protocol::{
    AgentSessionId, CompactString, ElicitationRequest, PermissionKind, PermissionOption, RawPayload,
};
use serde::Deserialize;
use serde_json::Value;

use crate::event::{AcpEvent, ProtocolWarning};
use crate::map;
use crate::state::{PendingElicitation, PendingPermission, Shared};

/// The SDK handler.
pub(crate) struct Inbound {
    shared: Arc<Shared>,
    /// Reused for each update's events, so mapping a chunk allocates no list.
    events: Vec<AcpEvent>,
}

impl HandleDispatchFrom<Agent> for Inbound {
    async fn handle_dispatch_from(
        &mut self,
        message: Dispatch,
        _connection: ConnectionTo<Agent>,
    ) -> Result<Handled<Dispatch>, agent_client_protocol::Error> {
        match message {
            Dispatch::Notification(message) => {
                self.notification(message).await;
                Ok(Handled::Yes)
            }
            Dispatch::Request(message, responder) => {
                self.request(message, responder).await;
                Ok(Handled::Yes)
            }
            response @ Dispatch::Response(..) => Ok(Handled::No {
                message: response,
                retry: false,
            }),
        }
    }

    fn describe_chain(&self) -> impl std::fmt::Debug {
        "clauseo-acp"
    }
}

impl Inbound {
    pub(crate) fn new(shared: Arc<Shared>) -> Self {
        Self {
            shared,
            events: Vec::new(),
        }
    }

    async fn notification(&mut self, message: UntypedMessage) {
        match message.method.as_str() {
            "session/update" => self.session_update(message.params).await,
            "elicitation/complete" => {
                match acp::CompleteElicitationNotification::deserialize(&message.params) {
                    Ok(complete) => {
                        let elicitation_id = complete.elicitation_id.0.to_string();
                        let event = AcpEvent::ElicitationCompleted { elicitation_id };
                        self.shared.emit(event).await;
                    }
                    Err(error) => {
                        let warning = invalid_message("elicitation/complete", &error);
                        self.shared.emit(AcpEvent::Warning(warning)).await;
                    }
                }
            }
            crate::line::MALFORMED_LINE_METHOD => {
                let bytes = message.params.get("bytes").and_then(Value::as_u64);
                let bytes = bytes
                    .and_then(|bytes| usize::try_from(bytes).ok())
                    .unwrap_or_default();
                let warning = ProtocolWarning::MalformedLine { bytes };
                self.shared.emit(AcpEvent::Warning(warning)).await;
            }
            // `$/cancel_request` and extension notifications: nothing to do.
            _ => {}
        }
    }

    async fn session_update(&mut self, mut params: Value) {
        let Some(update) = params.get_mut("update") else {
            let warning = ProtocolWarning::InvalidUpdate {
                name: String::new(),
                error: "session/update without an update".to_owned(),
            };
            return self.shared.emit(AcpEvent::Warning(warning)).await;
        };
        let name = update.get("sessionUpdate").and_then(Value::as_str);
        let Some(name) = name.filter(|name| map::KNOWN_UPDATES.contains(name)) else {
            let name = name.unwrap_or_default();
            return self.shared.emit(map::unknown(name)).await;
        };
        let name: &'static str = map::KNOWN_UPDATES
            .into_iter()
            .find(|known| *known == name)
            .unwrap_or_default();
        let quarantined = map::quarantine_unknown_blocks(update);
        let notification = match serde_json::from_value::<acp::SessionNotification>(params) {
            Ok(notification) => notification,
            Err(error) => {
                return self
                    .shared
                    .emit(map::invalid(name, error.to_string()))
                    .await;
            }
        };
        let Ok(session) = AgentSessionId::new(&notification.session_id.0) else {
            let error = "empty sessionId".to_owned();
            return self.shared.emit(map::invalid(name, error)).await;
        };
        {
            let state = self.shared.state();
            let session_state = state.sessions.get(session.as_str());
            let origin = session_state.map_or(crate::event::Origin::Idle, |s| s.origin);
            let cwd = session_state.map(|s| s.cwd.as_path());
            map::update(
                &session,
                origin,
                notification.update,
                cwd,
                quarantined,
                &mut self.events,
            );
        }
        for event in self.events.drain(..) {
            self.shared.emit(event).await;
        }
    }

    async fn request(&mut self, message: UntypedMessage, responder: Responder<Value>) {
        let checked = match message.method.as_str() {
            "session/request_permission" => permission(message.params).map(Request::Permission),
            "elicitation/create" => elicitation(message.params).map(Request::Elicitation),
            // Clauseo advertises no file system and no terminal; the same for anything else.
            _ => Err(agent_client_protocol::Error::method_not_found()),
        };
        let request = match checked {
            Ok(request) => request,
            Err(error) => {
                // An error means the connection is closing; nobody is left to answer.
                match responder.respond_with_error(error) {
                    Ok(()) | Err(_) => {}
                }
                return;
            }
        };
        let event = {
            let mut state = self.shared.state();
            let key = state.next_key();
            match request {
                Request::Permission(request) => {
                    let pending = PendingPermission {
                        session: CompactString::from(request.session.as_str()),
                        options: request
                            .options
                            .iter()
                            .map(|option| option.option_id.clone())
                            .collect(),
                        responder,
                    };
                    state.permissions.insert(key, pending);
                    AcpEvent::PermissionRequested {
                        session: request.session,
                        key,
                        acp_tool_call_id: request.acp_tool_call_id,
                        options: request.options,
                    }
                }
                Request::Elicitation(request) => {
                    let url = matches!(request.request, ElicitationRequest::Url { .. });
                    state
                        .elicitations
                        .insert(key, PendingElicitation { url, responder });
                    AcpEvent::ElicitationRequested {
                        session: request.session,
                        key,
                        elicitation_id: request.elicitation_id,
                        request: request.request,
                    }
                }
            }
        };
        self.shared.emit(event).await;
    }
}

/// An agent request this crate answers through the host.
enum Request {
    Permission(PermissionRequest),
    Elicitation(CheckedElicitation),
}

/// A permission request, checked.
struct PermissionRequest {
    session: AgentSessionId,
    acp_tool_call_id: CompactString,
    options: Vec<PermissionOption>,
}

/// Checks a `session/request_permission`. Refuses (-32602) one that does not decode, that offers
/// an option of a kind this crate does not know, or whose option id no command could name.
fn permission(params: Value) -> Result<PermissionRequest, agent_client_protocol::Error> {
    let kinds = permission_kinds(&params)
        .map_err(|kind| invalid_params(&format!("unknown option kind {kind}")))?;
    let request = acp::RequestPermissionRequest::deserialize(params)
        .map_err(|error| invalid_params(&error.to_string()))?;
    let session = AgentSessionId::new(&request.session_id.0)
        .map_err(|error| invalid_params(&format!("sessionId: {error}")))?;
    let mut options = Vec::with_capacity(request.options.len());
    for (option, kind) in request.options.into_iter().zip(kinds) {
        let id = &*option.option_id.0;
        if let Err(error) = clauseo_protocol::AgentChoiceId::new(id) {
            return Err(invalid_params(&format!("option id {id:?}: {error}")));
        }
        options.push(PermissionOption {
            option_id: CompactString::from(id),
            kind,
            agent_label: option.name,
        });
    }
    Ok(PermissionRequest {
        session,
        acp_tool_call_id: CompactString::from(&*request.tool_call.tool_call_id.0),
        options,
    })
}

/// An elicitation request, checked.
struct CheckedElicitation {
    session: AgentSessionId,
    elicitation_id: Option<String>,
    request: ElicitationRequest,
}

/// Checks an `elicitation/create`. Refuses (-32602) one that does not decode, has a mode other
/// than form or URL, or is scoped to a request instead of a session.
fn elicitation(params: Value) -> Result<CheckedElicitation, agent_client_protocol::Error> {
    let request = acp::CreateElicitationRequest::deserialize(params)
        .map_err(|error| invalid_params(&error.to_string()))?;
    let message = request.message;
    let (scope, elicitation_id, request) = match request.mode {
        acp::ElicitationMode::Form(form) => {
            // The SDK keeps `properties` in a `BTreeMap`, so they arrive sorted by name.
            let schema = serde_json::to_value(&form.requested_schema)
                .and_then(RawPayload::deserialize)
                .map_err(|error| invalid_params(&format!("the schema: {error}")))?;
            (
                form.scope,
                None,
                ElicitationRequest::Form { message, schema },
            )
        }
        acp::ElicitationMode::Url(url) => {
            let id = Some(url.elicitation_id.0.to_string());
            (
                url.scope,
                id,
                ElicitationRequest::Url {
                    message,
                    url: url.url,
                },
            )
        }
        acp::ElicitationMode::Other(_) | _ => {
            return Err(invalid_params("only form and url elicitations"));
        }
    };
    let acp::ElicitationScope::Session(scope) = scope else {
        return Err(invalid_params("only elicitations scoped to a session"));
    };
    let session = AgentSessionId::new(&scope.session_id.0)
        .map_err(|error| invalid_params(&format!("sessionId: {error}")))?;
    Ok(CheckedElicitation {
        session,
        elicitation_id,
        request,
    })
}

fn invalid_params(detail: &str) -> agent_client_protocol::Error {
    agent_client_protocol::Error::invalid_params().data(detail.to_owned())
}

/// Each option's kind, read before the SDK decodes the request, or the first kind this crate
/// does not know. Kinds are read from the JSON because the SDK's own enum would refuse the whole
/// request with a decode error, which hides which option was wrong.
fn permission_kinds(params: &Value) -> Result<Vec<PermissionKind>, String> {
    let options = params.get("options").and_then(Value::as_array);
    options
        .into_iter()
        .flatten()
        .map(|option| match option.get("kind").and_then(Value::as_str) {
            Some("allow_once") => Ok(PermissionKind::AllowOnce),
            Some("allow_always") => Ok(PermissionKind::AllowAlways),
            Some("reject_once") => Ok(PermissionKind::RejectOnce),
            Some("reject_always") => Ok(PermissionKind::RejectAlways),
            other => Err(format!("{other:?}")),
        })
        .collect()
}

fn invalid_message(method: &str, error: &serde_json::Error) -> ProtocolWarning {
    ProtocolWarning::InvalidUpdate {
        name: method.to_owned(),
        error: error.to_string(),
    }
}
