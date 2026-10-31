//! The commands that answer the agent (`AnswerPermission`, `AnswerElicitation`) and change its
//! session (`SetConfigOption`, `SetMode`). An answer is logged first and reaches the agent once
//! its commit is durable.

use clauseo_acp::PendingKey;
use clauseo_protocol::{
    AgentChoiceId, CommandOutcome, CompactString, ConfigOption, ConfigOptionsReplaced,
    ElicitationId, ElicitationOutcome, ElicitationRequest, ElicitationResolved,
    ElicitationResponse, ErrorCode, ModeChanged, PermissionChoice, PermissionRequestId,
    PermissionResolved, Principal, RawPayload, StreamId, ThreadId,
};

use super::super::commands::{AfterCommand, CommandCtx};
use super::super::{Actor, Tag};
use super::out::Out;
use super::{Then, TurnTag, spawn, state};
use crate::error::Error;

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
    let then = Then::Permission {
        agent: agent.id,
        key,
        option: option_id,
    };
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    let resolved = PermissionResolved {
        request,
        choice: PermissionChoice::Selected {
            option_id: option.option_id,
            kind: option.kind,
        },
        by: Principal::LocalDesktop,
    };
    out.push(actor, resolved)?;
    send_answer(actor, ctx, thread, out, then)
}

/// Sends an answer's commit with its receipt; `then` tells the agent once it is durable.
fn send_answer(
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
            | Then::Undone
            | Then::Permission { .. }
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
            tracing::warn!(%thread, %error, "a permission answer did not reach the agent");
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
    {
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
    }
    let agent = state(actor, thread)
        .and_then(|state| state.agent.as_deref_mut())
        .ok_or_else(not_pending)?;
    let key = agent
        .elicitations
        .remove(&elicitation)
        .ok_or_else(not_pending)?;
    let outcome = match &response {
        ElicitationResponse::Accept { .. } => ElicitationOutcome::Accepted,
        ElicitationResponse::Decline => ElicitationOutcome::Declined,
        ElicitationResponse::Cancel => ElicitationOutcome::Cancelled,
    };
    let then = Then::Elicitation {
        agent: agent.id,
        key,
        response,
    };
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
            tracing::warn!(%thread, %error, "an elicitation answer did not reach the agent");
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

/// `SetConfigOption` returned: log every option as the agent now has them.
pub(super) fn on_config(
    actor: &mut Actor,
    thread: ThreadId,
    ctx: CommandCtx,
    same: bool,
    result: Result<Vec<ConfigOption>, clauseo_acp::Error>,
) {
    let done = answered(same, result).and_then(|options| {
        let mut out = Out::new(actor, thread, Some(ctx.id))?;
        out.push(actor, ConfigOptionsReplaced { options })?;
        send_answer(actor, ctx, thread, out, Then::Nothing)
    });
    if let Err(error) = done {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// `SetMode` returned: log the mode.
pub(super) fn on_mode(
    actor: &mut Actor,
    thread: ThreadId,
    ctx: CommandCtx,
    same: bool,
    mode: &AgentChoiceId,
    result: Result<(), clauseo_acp::Error>,
) {
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
