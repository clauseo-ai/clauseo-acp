//! A model switch that would cost a Claude session its `auto` mode is refused (audit row 4 of
//! b4l-sbxrefuse): the session goes back to the model it had and to `auto`, and the thread gets
//! a notice naming both models ([`NoticeKind::ModelSwitchRefused`]).
//!
//! claude-agent-acp 0.89.1 does not say which models can run auto (its model option carries no
//! `supportsAutoMode`, `session-model.js:163-210`), so the host only learns it from the switch:
//! for a model that cannot, the adapter moves the session to `acceptEdits` and sends a
//! `current_mode_update` before it answers the `session/set_config_option`
//! (`acp-agent.js:6788`, `:6891`; `session-mode.js:122-134`). So:
//!
//! - a `SetConfigOption` of the model, sent while auto is wanted outside forced ask, marks the
//!   agent `Auto::Switching` with the value the model had ([`sending`]);
//! - a move to `acceptEdits` meanwhile is noted ([`moved`]);
//! - when the answer comes and such a move came first, the old model is set again, then `auto`
//!   ([`put_back`]); the pick is not kept in the matter's prefs. Once both answer
//!   ([`on_kept`]), the restored options, `ModeChanged auto` and the notice go in the command's
//!   commit, with a warn line. The command itself is `Done`: the app shows no error for a pick
//!   and follows the options (`ui/app/src/room/config-menus.ts`).
//! - If either call fails, the session stays in `acceptEdits` with auto unavailable, and the
//!   thread gets the `AutoModeUnavailable` notice, with a warn line: never silently weaker.
//!
//! The lawyer can still pick any other model. A move to `acceptEdits` with no switch of
//! Clauseo's out (an "always allow" answer that sets the mode, or a model switch the CLI made
//! itself) is kept as before (`forced_mode.rs`), with a warn line.

use clauseo_protocol::{
    AgentChoiceId, AgentKind, CompactString, ConfigCategory, ConfigOption, ConfigOptionKind,
    ConfigOptionsReplaced, ConfigValue, ErrorCode, ModeChanged, Notice, NoticeKind, NoticeLevel,
    ThreadId,
};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::agents::{Agent, call};
use super::inputs::AgentReply;
use super::out::Out;
use super::picks::Pick;
use super::session::Auto;
use super::{Then, state};
use crate::agent::loads::{CLAUDE_FALLBACK_MODE, CLAUDE_MODE};
use crate::error::Error;
use crate::log_file::Cause;

/// What the switch back gives [`on_kept`].
#[derive(Debug)]
pub(crate) struct Kept {
    ctx: CommandCtx,
    /// The name of the model the lawyer picked, and of the one kept.
    names: (CompactString, CompactString),
    /// The options as the refused switch's answer gave them.
    moved: Vec<ConfigOption>,
    /// The answer to setting the old model again.
    back: Result<Vec<ConfigOption>, clauseo_acp::Error>,
    /// The answer to `session/set_mode auto`, sent once the old model is set.
    mode: Option<Result<(), clauseo_acp::Error>>,
}

/// The host English of the notice, for a view that does not know the kind.
pub(crate) fn notice_text(picked: &str, kept: &str) -> String {
    format!(
        "{picked} can't run in Claude's automatic mode, so Claude stays on {kept}. To change \
         the model, pick another one."
    )
}

/// `thread`'s agent, when it is Claude's.
fn claude_agent(actor: &mut Actor, thread: ThreadId) -> Option<&mut Agent> {
    let claude = actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Claude));
    let agent = state(actor, thread)?.agent.as_deref_mut()?;
    claude.then_some(agent)
}

/// A select option's current value.
fn current_value(option: &ConfigOption) -> Option<AgentChoiceId> {
    let ConfigOptionKind::Select { current_value, .. } = &option.kind else {
        return None;
    };
    AgentChoiceId::new(current_value.as_str()).ok()
}

/// The model option's value before a `SetConfigOption` of `option` in `logged` (the thread's
/// options), as a pick; `None` when `option` is not the model.
pub(crate) fn model_before(logged: &[ConfigOption], option: &AgentChoiceId) -> Option<Pick> {
    let found = logged
        .iter()
        .find(|offered| offered.id == option.as_str())?;
    if found.category != ConfigCategory::Model {
        return None;
    }
    Some((option.clone(), current_value(found)?))
}

/// The display name of `value` in `options`' `option`, else the value itself.
pub(crate) fn choice_name(
    options: &[ConfigOption],
    option: &AgentChoiceId,
    value: &str,
) -> CompactString {
    let choices = options
        .iter()
        .find(|offered| offered.id == option.as_str())
        .and_then(|offered| match &offered.kind {
            ConfigOptionKind::Select { options, .. } => Some(options),
            ConfigOptionKind::Boolean { .. } | ConfigOptionKind::Unknown { .. } => None,
        });
    choices
        .and_then(|choices| choices.iter().find(|choice| choice.value == value))
        .map_or_else(|| CompactString::from(value), |choice| choice.name.clone())
}

/// `options` with the mode option's current value set to `mode`: the adapter answered the
/// model's switch back before `session/set_mode`, so its list still shows `acceptEdits`.
pub(crate) fn with_mode(mut options: Vec<ConfigOption>, mode: &str) -> Vec<ConfigOption> {
    for option in &mut options {
        if option.category != ConfigCategory::Mode {
            continue;
        }
        if let ConfigOptionKind::Select { current_value, .. } = &mut option.kind {
            *current_value = CompactString::from(mode);
        }
    }
    options
}

/// A `SetConfigOption` of `option` goes to `thread`'s agent: a Claude agent outside forced ask
/// that wants auto, switching its model, is marked `Switching` with the model's value.
pub(super) fn sending(actor: &mut Actor, thread: ThreadId, option: &AgentChoiceId) {
    let from = super::picks::logged(actor, thread).and_then(|logged| model_before(logged, option));
    let (Some(from), Some(agent)) = (from, claude_agent(actor, thread)) else {
        return;
    };
    if !agent.forced_ask && agent.auto == Auto::Wanted {
        agent.auto = Auto::Switching { from, left: false };
    }
}

/// `thread`'s agent moved its session to `mode`: a move to `acceptEdits` during a model switch
/// is noted; one with auto wanted and no switch out is logged.
pub(super) fn moved(actor: &mut Actor, thread: ThreadId, mode: &str) {
    if mode != CLAUDE_FALLBACK_MODE {
        return;
    }
    let Some(agent) = claude_agent(actor, thread) else {
        return;
    };
    match &mut agent.auto {
        Auto::Switching { left, .. } => *left = true,
        Auto::Wanted if !agent.forced_ask => tracing::warn!(
            %thread,
            "a Claude session moved itself from auto to acceptEdits with no model switch of \
             Clauseo's out; it stays there"
        ),
        Auto::Wanted | Auto::Set { .. } | Auto::Unavailable => {}
    }
}

/// A `SetConfigOption` answered: the agent leaves `Switching`, and the model's old value comes
/// back when the agent moved the session to `acceptEdits` meanwhile.
pub(super) fn answered(actor: &mut Actor, thread: ThreadId) -> Option<Pick> {
    let agent = state(actor, thread)?.agent.as_deref_mut()?;
    let Auto::Switching { from, left } = std::mem::take(&mut agent.auto) else {
        return None;
    };
    left.then_some(from)
}

/// The switch of `picked` moved the session off auto (its answer gave `options`): sets `from`
/// again, then `auto`; [`on_kept`] commits the outcome with the command's receipt.
pub(super) fn put_back(
    actor: &mut Actor,
    thread: ThreadId,
    (ctx, picked): (CommandCtx, &ConfigValue),
    options: Vec<ConfigOption>,
    from: Pick,
) {
    let picked = match picked {
        ConfigValue::Select(value) => value.as_str(),
        ConfigValue::Boolean(_) => "",
    };
    let names = (
        choice_name(&options, &from.0, picked),
        choice_name(&options, &from.0, from.1.as_str()),
    );
    tracing::warn!(
        %thread,
        option = %from.0,
        picked,
        kept = %from.1,
        "a model switch moved Claude off auto mode; switching the model back"
    );
    let agent = state(actor, thread).and_then(|state| state.agent.as_deref());
    let found =
        agent.and_then(|agent| Some((agent.id, agent.link.client.clone(), agent.session.clone()?)));
    let (Some((id, client, session)), Ok(auto)) = (found, AgentChoiceId::new(CLAUDE_MODE)) else {
        let error = Error::refused(ErrorCode::AgentExited, "the agent's session is gone");
        actor.finish_command(ctx.id, Err(error));
        return;
    };
    super::idle::call_started(actor, thread);
    call(actor, thread, id, async move {
        let value = ConfigValue::Select(from.1.clone());
        let back = client
            .set_config_option(&session, from.0.clone(), value)
            .await;
        let mode = match back {
            Ok(_) => Some(client.set_mode(&session, auto).await),
            Err(_) => None,
        };
        let kept = Kept {
            ctx,
            names,
            moved: options,
            back,
            mode,
        };
        AgentReply::ModelKept(Box::new(kept))
    });
}

/// The switch back answered. Both calls done: the restored options, `ModeChanged auto` and the
/// notice. Either failed: auto is unavailable, and the thread gets that notice instead.
pub(super) fn on_kept(actor: &mut Actor, thread: ThreadId, same: bool, kept: Kept) {
    let Kept {
        ctx,
        names: (picked, kept_name),
        moved,
        back,
        mode,
    } = kept;
    if !same {
        let error = Error::refused(
            ErrorCode::AgentExited,
            "the agent stopped before it answered",
        );
        actor.finish_command(ctx.id, Err(error));
        return;
    }
    let done = match (back, mode) {
        (Ok(options), Some(Ok(()))) => {
            let notice = Notice {
                level: NoticeLevel::Warning,
                text: notice_text(&picked, &kept_name),
                kind: Some(NoticeKind::ModelSwitchRefused {
                    picked: picked.to_string(),
                    kept: kept_name.to_string(),
                }),
            };
            let events = [
                ConfigOptionsReplaced {
                    options: with_mode(options, CLAUDE_MODE),
                }
                .into(),
                ModeChanged {
                    mode_id: CompactString::from(CLAUDE_MODE),
                }
                .into(),
                notice.into(),
            ];
            commit(actor, thread, ctx, events)
        }
        (back, mode) => {
            let error = match (&back, &mode) {
                (Err(error), _) | (Ok(_), Some(Err(error))) => Cause(error).to_string(),
                (Ok(_), None | Some(Ok(()))) => String::new(),
            };
            tracing::warn!(
                %thread,
                error,
                "Claude's model or mode was not switched back; it runs in acceptEdits"
            );
            let options = back.unwrap_or(moved);
            let notice = unavailable(actor, thread);
            commit(
                actor,
                thread,
                ctx,
                [ConfigOptionsReplaced { options }.into(), notice.into()],
            )
        }
    };
    if let Err(error) = done {
        actor.finish_command(ctx.id, Err(error));
    }
}

/// Marks `thread`'s agent's auto unavailable, and gives the notice that says so (told again,
/// even if the thread was told before in this run).
fn unavailable(actor: &mut Actor, thread: ThreadId) -> Notice {
    if let Some(state) = state(actor, thread) {
        state.auto_told = true;
        if let Some(agent) = state.agent.as_deref_mut() {
            agent.auto = Auto::Unavailable;
        }
    }
    Notice {
        level: NoticeLevel::Info,
        text: super::session::AUTO_UNAVAILABLE.to_owned(),
        kind: Some(NoticeKind::AutoModeUnavailable),
    }
}

/// Commits `events` with the command's receipt.
fn commit<const N: usize>(
    actor: &mut Actor,
    thread: ThreadId,
    ctx: CommandCtx,
    events: [clauseo_protocol::DomainEvent; N],
) -> Result<(), Error> {
    let mut out = Out::new(actor, thread, Some(ctx.id))?;
    for event in events {
        out.push(actor, event)?;
    }
    super::answers::send_answer(actor, ctx, thread, out, Then::Nothing)
}

#[cfg(test)]
mod tests;
