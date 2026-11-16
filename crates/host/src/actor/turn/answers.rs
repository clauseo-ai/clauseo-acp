//! The commands that answer the agent (`AnswerPermission`, `AnswerElicitation`) and change its
//! session (`SetConfigOption`, `SetMode`). An answer is logged first and reaches the agent once
//! its commit is durable.

use clauseo_acp::PendingKey;
use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, CompactString, ConfigOption, ConfigOptionsReplaced, ConfigValue,
    ElicitationId, ElicitationOutcome, ElicitationRequest, ElicitationResolved,
    ElicitationResponse, ErrorCode, ModeChanged, PermissionChoice, PermissionKind,
    PermissionRequestId, PermissionResolved, Principal, RawPayload, StopReason, StreamId, ThreadId,
};

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::{Actor, Tag, limits};
use super::out::Out;
use super::{Then, TurnTag, current, spawn, state};
use crate::error::Error;
use crate::log_file::Cause;

/// `AnswerPermission`: the request must be pending and the option one the agent offered.
pub(super) fn answer_permission(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    request: PermissionRequestId,
    option_id: AgentChoiceId,
) -> Result<(), Error> {
    let not_pending = || {
        Error::refused(
            ErrorCode::PermissionNotPending,
            format!("permission request {request} is not pending"),
        )
    };
    let option = {
        let pending = actor
            .loaded(StreamId::Thread(thread))
            .and_then(|loaded| loaded.sent.permission(request))
            .ok_or_else(not_pending)?;
        pending
            .options
            .iter()
            .find(|option| option.option_id.as_str() == option_id.as_str())
            .cloned()
            .ok_or_else(|| {
                Error::refused(
                    ErrorCode::PermissionUnknownOption,
                    format!("the agent did not offer option {option_id}"),
                )
            })?
    };
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref_mut())
        .ok_or_else(not_pending)?;
    let key = agent.permissions.remove(&request).ok_or_else(not_pending)?;
    let refused = matches!(
        option.kind,
        PermissionKind::RejectOnce | PermissionKind::RejectAlways
    )
    .then_some(StopReason::NotAllowed);
    let then = Then::Permission {
        agent: agent.id,
        key,
        option: option_id,
    };
    limits::asking_changed(actor);
    super::offline::ask_ended(actor, thread);
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    let resolved = PermissionResolved {
        request,
        choice: PermissionChoice::Selected {
            option_id: option.option_id,
            kind: option.kind,
        },
        by: Principal::LocalDesktop,
        held_open: None,
    };
    out.push(actor, resolved)?;
    send_answer(actor, ctx, thread, out, then)?;
    // The lawyer's last answer decides how an agent's own cancel of the turn reads.
    if let Some(current) = current(actor, thread) {
        current.refused = refused;
    }
    Ok(())
}

/// Sends an answer's commit with its receipt; `then` tells the agent once it is durable.
pub(super) fn send_answer(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    mut out: Out,
    then: Then,
) -> Result<(), Error> {
    let now = actor.now().wall;
    let outcome = CommandOutcome::Done;
    out.draft
        .receipt(ctx.receipt(Some(StreamId::Thread(thread)), outcome, now));
    let tag = Tag::Command {
        id: ctx.id,
        outcome,
        after: match then {
            Then::Nothing => AfterCommand::Nothing,
            then @ (Then::Launch(_)
            | Then::Started(_)
            | Then::Closing(_)
            | Then::LimitStop(_)
            | Then::Undone
            | Then::Queue(_)
            | Then::Permission { .. }
            | Then::CancelPermission { .. }
            | Then::Elicitation { .. }) => AfterCommand::Turn(TurnTag { thread, then }),
        },
    };
    out.send_tagged(actor, tag)
}

/// The permission answer is durable: the agent hears it.
pub(super) fn tell_permission(
    actor: &Actor,
    thread: ThreadId,
    agent: u64,
    key: PendingKey,
    option: AgentChoiceId,
) {
    let Some(client) = client_of(actor, thread, agent) else {
        return;
    };
    spawn(actor, async move {
        if let Err(error) = client.answer_permission(key, option).await {
            tracing::warn!(
                %thread,
                error = %Cause(&error),
                "a permission answer did not reach the agent"
            );
        }
    });
}

/// The host's cancel of a permission request is durable: the agent hears it.
pub(super) fn tell_cancel_permission(actor: &Actor, thread: ThreadId, agent: u64, key: PendingKey) {
    let Some(client) = client_of(actor, thread, agent) else {
        return;
    };
    spawn(actor, async move {
        if let Err(error) = client.cancel_permission(key).await {
            tracing::warn!(
                %thread,
                error = %Cause(&error),
                "a permission cancel did not reach the agent"
            );
        }
    });
}

/// `AnswerElicitation`: the elicitation must be pending, and a form's values must match the
/// types its schema gives.
pub(super) fn answer_elicitation(
    actor: &mut Actor,
    ctx: CommandCtx,
    thread: ThreadId,
    elicitation: ElicitationId,
    response: ElicitationResponse,
) -> Result<(), Error> {
    let not_pending = || {
        Error::refused(
            ErrorCode::ElicitationNotPending,
            format!("elicitation {elicitation} is not pending"),
        )
    };
    let form = {
        let request = actor
            .loaded(StreamId::Thread(thread))
            .and_then(|loaded| loaded.sent.elicitation(elicitation))
            .ok_or_else(not_pending)?;
        if let (ElicitationResponse::Accept { values }, ElicitationRequest::Form { schema, .. }) =
            (&response, request)
            && !values_match(schema, values)
        {
            return Err(Error::refused(
                ErrorCode::ElicitationInvalidValues,
                "the values do not match the form's schema",
            ));
        }
        matches!(request, ElicitationRequest::Form { .. })
    };
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref_mut())
        .ok_or_else(not_pending)?;
    let key = agent
        .elicitations
        .remove(&elicitation)
        .ok_or_else(not_pending)?;
    if !matches!(response, ElicitationResponse::Accept { .. }) {
        // Only an accepted URL elicitation goes on in the browser.
        agent.url_elicitations.retain(|_, id| *id != elicitation);
    }
    let agent = agent.id;
    // An accepted form's strings are withheld from the tool output that follows (0002,
    // `elicitation_resolved`), before the agent can echo them.
    if let Some(current) = current(actor, thread) {
        match &response {
            ElicitationResponse::Accept { values } if form => {
                current.withheld.accepted(elicitation, values);
            }
            ElicitationResponse::Accept { .. }
            | ElicitationResponse::Decline
            | ElicitationResponse::Cancel => current.withheld.forget(elicitation),
        }
    }
    let outcome = match &response {
        ElicitationResponse::Accept { .. } => ElicitationOutcome::Accepted,
        ElicitationResponse::Decline => ElicitationOutcome::Declined,
        ElicitationResponse::Cancel => ElicitationOutcome::Cancelled,
    };
    let then = Then::Elicitation {
        agent,
        key,
        response,
    };
    limits::asking_changed(actor);
    super::offline::ask_ended(actor, thread);
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    let resolved = ElicitationResolved {
        elicitation,
        outcome,
        by: Principal::LocalDesktop,
    };
    out.push(actor, resolved)?;
    send_answer(actor, ctx, thread, out, then)
}

/// The elicitation answer is durable: the agent hears it.
pub(super) fn tell_elicitation(
    actor: &Actor,
    thread: ThreadId,
    agent: u64,
    key: PendingKey,
    response: ElicitationResponse,
) {
    let Some(client) = client_of(actor, thread, agent) else {
        return;
    };
    spawn(actor, async move {
        if let Err(error) = client.answer_elicitation(key, response).await {
            tracing::warn!(
                %thread,
                error = %Cause(&error),
                "an elicitation answer did not reach the agent"
            );
        }
    });
}

fn client_of(actor: &Actor, thread: ThreadId, agent: u64) -> Option<clauseo_acp::AcpClient> {
    let found = actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.agent.as_deref())
        .filter(|held| held.id == agent)
        .map(|held| held.link.client.clone());
    if found.is_none() {
        tracing::debug!(%thread, "an answer for an agent that is gone");
    }
    found
}

/// Whether a form's `values` are an object whose every property the schema names with a
/// matching type, and which holds every property the schema requires.
fn values_match(schema: &RawPayload, values: &RawPayload) -> bool {
    let RawPayload::Map(given) = values else {
        return false;
    };
    let properties = field(schema, "properties");
    for (key, value) in &**given {
        let RawPayload::Str(name) = key else {
            return false;
        };
        let Some(property) = properties.and_then(|properties| field(properties, name)) else {
            return false;
        };
        let Some(RawPayload::Str(kind)) = field(property, "type") else {
            continue;
        };
        let fits = match &**kind {
            "string" => matches!(value, RawPayload::Str(_)),
            "number" => matches!(
                value,
                RawPayload::I64(_) | RawPayload::U64(_) | RawPayload::F64(_)
            ),
            "integer" => matches!(value, RawPayload::I64(_) | RawPayload::U64(_)),
            "boolean" => matches!(value, RawPayload::Bool(_)),
            "array" => matches!(value, RawPayload::Seq(_)),
            "object" => matches!(value, RawPayload::Map(_)),
            "null" => matches!(value, RawPayload::Null),
            _ => true,
        };
        if !fits {
            return false;
        }
    }
    let Some(RawPayload::Seq(required)) = field(schema, "required") else {
        return true;
    };
    required.iter().all(|name| match name {
        RawPayload::Str(name) => given
            .iter()
            .any(|(key, _)| matches!(key, RawPayload::Str(key) if key == name)),
        RawPayload::Null
        | RawPayload::Bool(_)
        | RawPayload::I64(_)
        | RawPayload::U64(_)
        | RawPayload::F64(_)
        | RawPayload::Bytes(_)
        | RawPayload::Seq(_)
        | RawPayload::Map(_) => true,
    })
}

/// The value under `name` in a JSON object.
fn field<'a>(object: &'a RawPayload, name: &str) -> Option<&'a RawPayload> {
    let RawPayload::Map(entries) = object else {
        return None;
    };
    entries
        .iter()
        .find(|(key, _)| matches!(key, RawPayload::Str(key) if &**key == name))
        .map(|(_, value)| value)
}

/// `SetConfigOption` of `pick` returned: log every option as the agent now has them, and keep
/// the pick in the matter's prefs (`picks.rs`), in the same commit.
pub(super) fn on_config(
    actor: &mut Actor,
    thread: ThreadId,
    (ctx, pick): (CommandCtx, (AgentChoiceId, ConfigValue)),
    same: bool,
    result: Result<Vec<ConfigOption>, clauseo_acp::Error>,
) {
    // A model switch that moved Claude off auto is put back (`model_switch.rs`).
    let left = super::model_switch::answered(actor, thread);
    let done = answered(same, result).and_then(|options| {
        if let Some(from) = left {
            super::model_switch::put_back(actor, thread, (ctx, &pick.1), options, from);
            return Ok(());
        }
        let mut out = Out::new(actor, thread, Some(ctx.id))?;
        out.push(actor, ConfigOptionsReplaced { options })?;
        if let Some(row) = super::picks::remember(actor, thread, &pick) {
            out.draft.matter_row(row);
        }
        send_answer(actor, ctx, thread, out, Then::Nothing)
    });
    if let Err(error) = done {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// `SetMode` to the attached agent of `thread`: refused in forced ask for a mode that approves
/// changes unasked, else `session/set_mode`, whose answer waits for the link's fence so the
/// agent's reports of its mode come first (`forced_mode/read_back.rs`).
pub(super) fn send_mode(actor: &mut Actor, thread: ThreadId, ctx: CommandCtx, mode: AgentChoiceId) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref()) else {
        actor.finish_command(ctx.id, Err(super::lost("the agent")));
        return;
    };
    if agent.forced_ask && !crate::agent::forced_ask::mode_allowed(mode.as_str()) {
        let error = Error::refused(
            ErrorCode::CommandInvalid,
            format!(
                "mode {mode} would approve changes without asking, and this matter's folder is \
                 unprotected: only \"default\" and \"plan\" may be set"
            ),
        );
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let (id, client, session) = (agent.id, agent.link.client.clone(), agent.session.clone());
    let (mark, fence) = (agent.modes.mark(), agent.link.fence());
    let Some(session) = session else {
        actor.finish_command(ctx.id, Err(super::lost("the agent's session")));
        return;
    };
    super::idle::call_started(actor, thread);
    super::agents::call(actor, thread, id, async move {
        let result = client.set_mode(&session, mode.clone()).await;
        fence.await;
        super::AgentReply::Mode {
            ctx,
            mode,
            mark,
            result,
        }
    });
}

/// `SetMode` returned: once a Claude agent reported the mode (`forced_mode/read_back.rs`), log
/// it; another mode reported fails the command with `agent.mode_not_set` and stops the
/// conversation.
pub(super) fn on_mode(
    actor: &mut Actor,
    thread: ThreadId,
    (ctx, same): (CommandCtx, bool),
    (mode, mark): (&AgentChoiceId, super::forced_mode::read_back::Mark),
    result: Result<(), clauseo_acp::Error>,
) {
    use super::forced_mode::read_back;
    if result.is_ok()
        && same
        && let Err(error) = read_back::check(actor, thread, mode.as_str(), mark)
    {
        read_back::stop(actor, thread, &error);
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let done = answered(same, result).and_then(|()| {
        let mut out = Out::new(actor, thread, Some(ctx.id))?;
        let mode_id = CompactString::from(mode.as_str());
        out.push(actor, ModeChanged { mode_id })?;
        send_answer(actor, ctx, thread, out, Then::Nothing)
    });
    if let Err(error) = done {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// A session call's result, or `agent.exited` when the agent that answered is gone.
fn answered<T>(same: bool, result: Result<T, clauseo_acp::Error>) -> Result<T, Error> {
    if !same {
        return Err(Error::refused(
            ErrorCode::AgentExited,
            "the agent stopped before it answered",
        ));
    }
    result.map_err(Error::Acp)
}
