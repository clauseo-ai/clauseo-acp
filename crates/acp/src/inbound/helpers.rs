//! What [`Inbound`](super::Inbound) does with codex-acp's helper sessions
//! (`ConnectOptions::helper_sessions`): the `subagent_*` updates, and the updates under a
//! helper's own session id. The mapping is in `map/helpers.rs`; which session is a helper's, in
//! `state/helper_sessions.rs`. Each function pushes its events onto `events`, which the caller
//! sends.

use agent_client_protocol::schema::v1 as acp;
use clauseo_protocol::{AgentSessionId, CompactString};
use serde_json::Value;

use crate::event::{AcpEvent, HelperState, Origin, ProtocolWarning};
use crate::map::{self, Quarantined, helpers::Subagent};
use crate::state::Shared;

/// An update under an announced helper's session id: its root, and what the mapping needs from
/// the raw JSON.
pub(super) struct Routed {
    root: AgentSessionId,
    /// `map::helpers::final_answer` of the raw update.
    final_answer: bool,
    /// `map::helpers::failure_warning` of the raw update.
    failure: ProtocolWarning,
}

/// Where an update on `session` goes when `session` is an announced helper's. `None` for any
/// other session, and always on a connection without helper sessions: the update then maps as
/// it did before. `update` is the raw `update` object.
pub(super) fn route(
    shared: &Shared,
    session: Option<&AgentSessionId>,
    update: &Value,
) -> Option<Routed> {
    if !shared.helper_sessions {
        return None;
    }
    let root = shared.state().helpers.root_of(session?.as_str())?.clone();
    Some(Routed {
        root,
        final_answer: map::helpers::final_answer(update),
        failure: map::helpers::failure_warning(update),
    })
}

/// `session`, or its root when it is an announced helper's; without helper sessions,
/// `session` at once, with no look at the state.
pub(super) fn root_of(shared: &Shared, session: AgentSessionId) -> AgentSessionId {
    if shared.helper_sessions {
        shared.state().root_of(session)
    } else {
        session
    }
}

/// Maps helper session `helper`'s update as the same update on its root session would map,
/// with the root's origin and folder, then makes the events the helper's
/// (`map::helpers::helper_events`). It reads no other state of the root and changes none: a
/// plan update sees no shown plan and keeps none.
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
        let cwd = root.map(|root| root.cwd.as_path());
        let mut no_plan = None;
        let place = (cwd, &mut no_plan);
        map::update(routed.root, origin, update, place, quarantined, events);
    }
    map::helpers::helper_events(helper, routed.final_answer, &routed.failure, events);
}

/// A `subagent_spawned` or `subagent_state_update` (`name`) on `session`, read from the raw
/// `update`. The root is `session`'s own root when `session` is an announced helper's (the
/// helper is then the parent), else `session` itself, known to this connection or not; the
/// events carry the root's origin. A `subagent_spawned` records its helper under that root; it
/// only warns when the root is not a session this connection started, when it names `session`
/// or such a session, and when the helper was already announced under another root, which it
/// keeps.
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
    {
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
        match read {
            Subagent::Spawned {
                helper,
                name: label,
                task,
            } => {
                let known = state.helpers.root_of(helper.as_str());
                let refused = if !state.sessions.contains_key(root.as_str()) {
                    Some("a subagent_spawned on a session this connection did not start")
                } else if helper == session || state.sessions.contains_key(helper.as_str()) {
                    Some("a subagentSessionId that names a session of this connection")
                } else if known.is_some_and(|known| *known != root) {
                    Some("a subagentSessionId already announced under another session")
                } else {
                    None
                };
                if let Some(error) = refused {
                    let warning = map::invalid(Some(session), Some(name), error.to_owned());
                    events.push(warning);
                } else {
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
            }
            Subagent::Ended {
                helper,
                state: ended,
            } => {
                let ended = match ended {
                    Ok(ended) => ended,
                    Err(error) => {
                        let warning = map::invalid(Some(session), Some(name), error);
                        events.push(warning);
                        HelperState::Other
                    }
                };
                events.push(AcpEvent::HelperEnded {
                    session: root,
                    origin,
                    helper: CompactString::from(helper.as_str()),
                    state: ended,
                });
            }
        }
    }
}
