//! The connector state out to the app: `ConnectorsChanged` when it changes and in every app
//! catch-up, and `AddonsChanged` when case law's status in the skills and tools list moves.

use clauseo_connectors::CASE_LAW;
use clauseo_protocol::{ConnectorsState, HostEvent};

use super::recheck;
use crate::actor::Actor;

/// The state as the app sees it now, if connectors are wired.
fn view(actor: &Actor) -> Option<ConnectorsState> {
    actor.connectors.wired.as_ref()?;
    let available = actor.effective.settings().tools.connectors;
    let locked = actor.effective.is_locked("tools.connectors");
    Some(actor.connectors.state.view(&CASE_LAW, available, locked))
}

/// Sends the state to every app subscriber when it changed since the last one sent, and
/// `AddonsChanged` when case law's status in the skills and tools list moved.
pub(super) fn publish(actor: &mut Actor) {
    if let Some(state) = view(actor)
        && actor.connectors.published.as_ref() != Some(&state)
    {
        actor.connectors.published = Some(state.clone());
        crate::actor::app::publish(actor, &HostEvent::ConnectorsChanged(state));
    }
    recheck::changed(actor);
}

/// The app catch-up's part: the whole state, when connectors are wired.
pub(in crate::actor) fn current(actor: &Actor) -> Option<HostEvent> {
    view(actor).map(HostEvent::ConnectorsChanged)
}
