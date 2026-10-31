//! The agent's asks and signals: permission requests and elicitations (logged `Live` during
//! the live turn, else answered cancelled with nothing logged) and limit signals (a rejection
//! or a stop marks the live turn; the first wins).

use clauseo_acp::{LimitSignal, LimitStatus, Origin, PendingKey};
use clauseo_protocol::{
    AgentSessionId, CompactString, Deliverable, ElicitationId, ElicitationRequest,
    ElicitationRequested, ElicitationResponse, LimitStopReason, PermissionOption,
    PermissionRequestId, PermissionRequested, ThreadId,
};

use super::super::Actor;
use super::updates::{agent, commit, known_tool};
use super::{LimitMark, current, spawn};
use crate::error::Error;

/// A permission request: logged `Live` during the live turn, else answered `cancelled`.
pub(super) fn on_permission(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    key: PendingKey,
    acp_tool_call_id: &str,
    options: Vec<PermissionOption>,
) {
    let live = current(actor, thread).is_some_and(|current| current.live());
    if !live {
        if let Some(agent) = agent(actor, thread) {
            let client = agent.link.client.clone();
            let session = session.clone();
            spawn(actor, async move {
                // `cancel` answers every pending permission of the session `cancelled`.
                if let Err(error) = client.cancel(&session).await {
                    tracing::debug!(%error, "a permission outside a turn was not cancelled");
                }
            });
        }
        return;
    }
    let tool_call = known_tool(actor, thread, session, acp_tool_call_id);
    let logged = (|| -> Result<(), Error> {
        let request: PermissionRequestId = actor.ids.mint(actor.clock.now().wall)?;
        if let Some(agent) = agent(actor, thread) {
            agent.permissions.insert(request, key);
        }
        let event = PermissionRequested {
            request,
            tool_call,
            options,
            deliverable: Deliverable::Live,
        };
        commit(actor, thread, [event.into()], Vec::new())
    })();
    if let Err(error) = logged {
        tracing::warn!(%thread, %error, "a permission request was not logged");
    }
}

/// An elicitation: logged during the live turn, else answered `Cancel`.
pub(super) fn on_elicitation(
    actor: &mut Actor,
    thread: ThreadId,
    _session: &AgentSessionId,
    key: PendingKey,
    elicitation_id: Option<CompactString>,
    request: ElicitationRequest,
) {
    let live = current(actor, thread).is_some_and(|current| current.live());
    if !live {
        if let Some(agent) = agent(actor, thread) {
            let client = agent.link.client.clone();
            spawn(actor, async move {
                if let Err(error) = client
                    .answer_elicitation(key, ElicitationResponse::Cancel)
                    .await
                {
                    tracing::debug!(%error, "an elicitation outside a turn was not cancelled");
                }
            });
        }
        return;
    }
    let logged = (|| -> Result<(), Error> {
        let elicitation: ElicitationId = actor.ids.mint(actor.clock.now().wall)?;
        if let Some(agent) = agent(actor, thread) {
            agent.elicitations.insert(elicitation, key);
            if let Some(acp) = elicitation_id {
                agent.url_elicitations.insert(acp, elicitation);
            }
        }
        let event = ElicitationRequested {
            elicitation,
            request,
        };
        commit(actor, thread, [event.into()], Vec::new())
    })();
    if let Err(error) = logged {
        tracing::warn!(%thread, %error, "an elicitation was not logged");
    }
}

/// A limit signal: a rejection or a stop during the turn marks it (the first wins); anything
/// else goes to `tracing`.
pub(super) fn on_limit(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    origin: Origin,
    signal: &LimitSignal,
) {
    let mark = limit_mark(signal);
    let turn = match origin {
        Origin::Turn(turn) => Some(turn),
        Origin::Late(_) | Origin::Replay | Origin::Idle => None,
    };
    let current = current(actor, thread).filter(|current| Some(current.turn) == turn);
    if let (Some(current), Some(mark)) = (current, mark) {
        if current.limit.is_none() {
            current.limit = Some(mark);
        }
    } else {
        tracing::info!(%thread, %session, ?signal, "a limit signal");
    }
}

/// The limit stop a signal means, if it stops the turn.
pub(super) fn limit_mark(signal: &LimitSignal) -> Option<LimitMark> {
    match signal {
        LimitSignal::RateLimit {
            window,
            status,
            resets_at,
            ..
        } => match status {
            LimitStatus::Rejected => Some(LimitMark {
                window: window.clone(),
                resets_at: *resets_at,
                reason: LimitStopReason::PlanWindow,
            }),
            LimitStatus::Allowed | LimitStatus::Warning => None,
        },
        LimitSignal::Stopped { reason, .. } => Some(LimitMark {
            window: None,
            resets_at: None,
            reason: *reason,
        }),
    }
}
