//! What [`Inbound`](super::Inbound) does with codex-acp's helper sessions
//! (`ConnectOptions::helper_sessions`): the `subagent_*` updates, and the updates and requests
//! under a helper's own session id. The mapping is in `map/helpers.rs`; which session is a
//! helper's, in `state/helper_sessions.rs`. Each function pushes its events onto `events`, which
//! the caller sends.
//!
//! What a helper sends after its end, whatever the state (`completed`, `cancelled`, `failed`,
//! `disconnected`), is not the root's current turn's, even when it arrives while the root runs
//! a later turn. Its updates come as the lead's own do after a cancel: tagged `Late` with the
//! turn it ended in, which the host drops. When it ended outside a turn there is no turn to
//! name, and they are dropped with a warning. Its permission requests and elicitations are
//! answered `cancelled` here, with a warning; the host never sees them. In the 18 recordings,
//! over 32 ends (23 `completed`, 7 `cancelled`, 2 `disconnected`), the agent sends nothing on
//! a helper's session after its end until a new process announces it again (F1d's replay), so
//! none of this was seen from the agent.

use agent_client_protocol::Responder;
use agent_client_protocol::schema::v1 as acp;
use clauseo_protocol::{AgentSessionId, CompactString};
use serde_json::Value;

use super::Request;
use crate::event::{AcpEvent, Origin, ProtocolWarning};
use crate::map::{self, Quarantined, helpers::Subagent};
use crate::state::{HelperEnd, Shared, State};

/// What an ended helper's dropped update warns.
const ENDED_UPDATE: &str = "an update of a helper that ended outside a turn";

/// What an ended helper's refused request warns.
const ENDED_REQUEST: &str = "a request of a helper that has ended, answered cancelled";

/// An update under an announced helper's session id: its root, and what the mapping needs from
/// the raw JSON.
pub(super) struct Routed {
    root: AgentSessionId,
    /// How the helper ended, if it did.
    end: Option<HelperEnd>,
    /// The update's `sessionUpdate`, for a warning.
    name: &'static str,
    /// `map::helpers::final_answer` of the raw update.
    final_answer: bool,
    /// `map::helpers::failure_warning` of the raw update.
    failure: ProtocolWarning,
}

/// Where an update named `name` on `session` goes when `session` is an announced helper's.
/// `None` for any other session, and always on a connection without helper sessions: the update
/// then maps as it did before. `update` is the raw `update` object.
pub(super) fn route(
    shared: &Shared,
    session: Option<&AgentSessionId>,
    (name, update): (&'static str, &Value),
) -> Option<Routed> {
    if !shared.helper_sessions {
        return None;
    }
    let session = session?.as_str();
    let (root, end) = {
        let state = shared.state();
        let root = state.helpers.root_of(session)?.clone();
        (root, state.helpers.end_of(session))
    };
    Some(Routed {
        root,
        end,
        name,
        final_answer: map::helpers::final_answer(update),
        failure: map::helpers::failure_warning(update),
    })
}

/// The origin of a helper's update, given its root's origin and the helper's end: the root's
/// while it runs; `Late` of the turn it ended in after its end; `None` (drop it) after an end
/// outside a turn.
fn update_origin(root: Origin, end: Option<HelperEnd>) -> Option<Origin> {
    match end {
        None => Some(root),
        Some(HelperEnd(turn)) => turn.map(Origin::Late),
    }
}

/// The session and origin of the `UpdateSkipped` that stands for an update on `session`
/// dropped over the line cap: `session` and its origin, or for an announced helper's, its root
/// and the origin the helper's updates get (`update_origin`). The warning to send instead when
/// that update would be dropped.
pub(super) fn skipped(
    shared: &Shared,
    session: AgentSessionId,
) -> Result<(AgentSessionId, Origin), Box<AcpEvent>> {
    let state = shared.state();
    let helper = shared
        .helper_sessions
        .then(|| state.helpers.root_of(session.as_str()))
        .flatten();
    let (root, end) = match helper {
        Some(root) => (root.clone(), state.helpers.end_of(session.as_str())),
        None => (session.clone(), None),
    };
    let known = state.sessions.get(root.as_str());
    let origin = known.map_or(Origin::Idle, |known| known.origin);
    match update_origin(origin, end) {
        Some(origin) => Ok((root, origin)),
        None => Err(Box::new(map::invalid(
            Some(session),
            None,
            ENDED_UPDATE.to_owned(),
        ))),
    }
}

/// Maps helper session `helper`'s update as the same update on its root session would map,
/// with the root's folder and the origin `update_origin` gives, then makes the events the
/// helper's (`map::helpers::helper_events`). It reads no other state of the root and changes
/// none: a plan update sees no shown plan and keeps none. An ended helper's update with no turn
/// to name gives only a warning.
pub(super) fn helper_update(
    shared: &Shared,
    helper: &AgentSessionId,
    routed: Routed,
    (update, quarantined): (acp::SessionUpdate, Vec<Quarantined>),
    events: &mut Vec<AcpEvent>,
) {
    {
        let state = shared.state();
        let root = state.sessions.get(routed.root.as_str());
        let origin = root.map_or(Origin::Idle, |root| root.origin);
        let Some(origin) = update_origin(origin, routed.end) else {
            let error = ENDED_UPDATE.to_owned();
            events.push(map::invalid(Some(helper.clone()), Some(routed.name), error));
            return;
        };
        let cwd = root.map(|root| root.cwd.as_path());
        let mut no_plan = None;
        let place = (cwd, &mut no_plan);
        map::update(routed.root, origin, update, place, quarantined, events);
    }
    map::helpers::helper_events(helper, routed.final_answer, &routed.failure, events);
}

/// Whether `request` comes from an announced helper that has ended.
pub(super) fn ended(state: &State, request: &Request) -> bool {
    let session = match request {
        Request::Permission(request) => &request.session,
        Request::Elicitation(request) => &request.session,
    };
    state.helpers.end_of(session.as_str()).is_some()
}

/// Answers an ended helper's request here: a permission request `cancelled`, an elicitation
/// `cancel`. Gives the warning the host gets instead of the request.
pub(super) fn refuse(request: Request, responder: Responder<Value>) -> AcpEvent {
    let (session, method, answer) = match request {
        Request::Permission(request) => (
            request.session,
            "session/request_permission",
            serde_json::json!({"outcome": {"outcome": "cancelled"}}),
        ),
        Request::Elicitation(request) => (
            request.session,
            "elicitation/create",
            serde_json::json!({"action": "cancel"}),
        ),
    };
    // An error means the connection is closing; nobody is left to answer.
    match responder.respond(answer) {
        Ok(()) | Err(_) => {}
    }
    map::invalid(Some(session), Some(method), ENDED_REQUEST.to_owned())
}

/// A `subagent_spawned` or `subagent_state_update` (`name`) on `session`, read from the raw
/// `update`. The root is `session`'s own root when `session` is an announced helper's (the
/// helper is then the parent), else `session` itself, known to this connection or not; the
/// events carry the root's origin.
///
/// A `subagent_spawned` records its helper under that root. It only warns when the root is not
/// a session this connection started, when it comes on a helper that has ended, when it names
/// `session` or such a session, when the helper was announced before (under this root or
/// another, which it keeps), and when the root has used its `MAX_PER_ROOT` (1024,
/// `state/helper_sessions.rs`) helpers since it was last started.
///
/// A `subagent_state_update` ends a helper announced under that root, once. It only warns for a
/// helper not announced under that root, for one that already ended, and for a state that is not
/// one of the four codex-acp sends. codex-acp 2.10.0 sends this update only for a final state,
/// once per helper session id: `finish` returns when the helper already has a `terminalState`
/// (`dist/index.js:37972-37974`), and Codex's `running`, `pendingInit` and `inProgress` map to
/// no state and send nothing (`terminalStateOf`, `terminalStateFromTurn`, `:38104-38130`). A
/// load's replay sends one end per helper too (`:55466-55489`). A helper given more work after
/// it ended comes back under a new session id, `<thread>:generation:<n>`, with a new
/// `subagent_spawned` (`reopen`, `:37989`). Every recording has one end per announcement. An
/// unknown state may be a later version's non-final one, so it leaves the helper running.
pub(super) fn subagent(
    shared: &Shared,
    session: Option<AgentSessionId>,
    (name, update): (&'static str, &Value),
    events: &mut Vec<AcpEvent>,
) {
    let Some(session) = session else {
        let error = "a missing or empty sessionId".to_owned();
        return events.push(map::invalid(None, Some(name), error));
    };
    let read = match map::helpers::subagent(name, update) {
        Ok(read) => read,
        Err(error) => return events.push(map::invalid(Some(session), Some(name), error)),
    };
    let mut state = shared.state();
    let (root, parent) = if let Some(root) = state.helpers.root_of(session.as_str()) {
        (root.clone(), Some(CompactString::from(session.as_str())))
    } else {
        (session.clone(), None)
    };
    let origin = state
        .sessions
        .get(root.as_str())
        .map_or(Origin::Idle, |root| root.origin);
    let warn = |error: &str| map::invalid(Some(session.clone()), Some(name), error.to_owned());
    match read {
        Subagent::Spawned {
            helper,
            name: label,
            task,
        } => {
            let known = state.helpers.root_of(helper.as_str());
            let refused = if !state.sessions.contains_key(root.as_str()) {
                Some("a subagent_spawned on a session this connection did not start")
            } else if state.helpers.end_of(session.as_str()).is_some() {
                // An ended helper launches nothing: its helper would run on into the root's
                // later turns as if the root had launched it there.
                Some("a subagent_spawned on a helper that has ended")
            } else if helper == session || state.sessions.contains_key(helper.as_str()) {
                Some("a subagentSessionId that names a session of this connection")
            } else if known.is_some_and(|known| *known != root) {
                Some("a subagentSessionId already announced under another session")
            } else if known.is_some() {
                Some("a subagentSessionId already announced")
            } else if state.helpers.full(root.as_str()) {
                Some("this session has used its 1024 helpers since it was last started")
            } else {
                None
            };
            if let Some(error) = refused {
                return events.push(warn(error));
            }
            let helper = CompactString::from(helper.as_str());
            state.helpers.insert(helper.clone(), root.clone());
            events.push(AcpEvent::HelperSpawned {
                session: root,
                origin,
                helper,
                parent,
                name: label,
                task,
            });
        }
        Subagent::Ended {
            helper,
            state: ended,
        } => {
            if state.helpers.root_of(helper.as_str()) != Some(&root) {
                let error = "a subagentSessionId not announced under this session";
                return events.push(warn(error));
            }
            if state.helpers.end_of(helper.as_str()).is_some() {
                return events.push(warn("a subagentSessionId that already ended"));
            }
            let ended = match ended {
                Ok(ended) => ended,
                Err(error) => return events.push(warn(&error)),
            };
            let end = HelperEnd(match origin {
                Origin::Turn(turn) | Origin::Late(turn) => Some(turn),
                Origin::Replay | Origin::Idle => None,
            });
            state.helpers.end(helper.as_str(), end);
            events.push(AcpEvent::HelperEnded {
                session: root,
                origin,
                helper: CompactString::from(helper.as_str()),
                state: ended,
            });
        }
    }
}
