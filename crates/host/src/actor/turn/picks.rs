//! The model and effort a thread's session runs with, as the pickers set them.
//!
//! - Only a select option whose category is `Model` or `ThoughtLevel` may be set ([`settable`]),
//!   as the thread's logged options say, else as its agent's kept ones (`agent_options.rs`);
//!   one that waits for a session is checked again against the options it starts with.
//!   Modes keep their own commands and rules, and every other option (Claude's `fast`) stays
//!   the agent's.
//! - Each session start (`session/new`, `session/load` or `session/resume`) takes [`wanted`]
//!   before its attach, so before any prompt: a `SetConfigOption` waiting for the session (a
//!   pick made while it starts) over everything else; then the thread's own values once a
//!   session of it has attached and logged options, since a reopened conversation keeps its
//!   model and effort and neither adapter restores both (claude-agent-acp 0.89.1 takes the
//!   effort from its settings, codex-acp 2.2.2 both from Codex's resume answer); else
//!   `CreateThread`'s config; else the matter's prefs for the agent's kind; else nothing, the
//!   agent's defaults. Options the agent reports while its first session is being created are
//!   its defaults, not the thread's: they never stand for the thread's own values, and the
//!   options updates held until the start's answer are dropped there ([`drop_held`]).
//!   [`apply`] sets each value that differs from the session's, the model first since the
//!   efforts offered follow it. A waiting `SetConfigOption` the start's picks cover is answered
//!   then, without another call ([`after`]). One made while they were being set is set in
//!   another round before the attach ([`late`]), so the first prompt runs with it too; any
//!   other goes to the agent at the attach.
//! - A pick the agent does not offer, or refuses as invalid (-32602), is dropped ([`after`]):
//!   logged at info with its option and value ids, taken out of the matter's prefs, and the
//!   session goes on with the agent's value. So is a pick of an option that is no model or
//!   effort one, or that `config_allowed` refuses. A pick of an option the session does not
//!   report (a load answer without options), or one the agent fails otherwise (a rate limit),
//!   is kept for a later session. A pick never fails or holds the launch, nor the prompt behind
//!   it.
//! - A `SetConfigOption` that succeeds keeps its pick in the matter's prefs for the thread's
//!   agent kind ([`remember`]).
//! - A prompt's own picks (`SendPrompt::config`) are set just before that prompt goes out
//!   (`prompt_picks.rs`).

use clauseo_acp::{AcpClient, AcpEvent, AgentUpdate, SessionStart};
use clauseo_protocol::{
    AgentChoiceId, AgentKind, ConfigCategory, ConfigOption, ConfigOptionKind, ConfigPicks,
    ConfigValue, ErrorCode, HeaderEntry, StreamId, ThreadId,
};
use clauseo_store::MatterRow;

use self::log::{model_and_effort, pick_value};
use super::super::Actor;
use super::agents::{Stage, Waiting};
use super::out::Out;
use super::{Then, state};
use crate::agent::forced_ask::config_allowed;
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

/// The JSON-RPC code of an agent refusing a value as invalid.
const INVALID_PARAMS: i32 = -32602;

/// An option's id and the id of the value to set.
pub(super) type Pick = (AgentChoiceId, AgentChoiceId);

/// What [`apply`] did, for [`after`].
#[derive(Debug, Default)]
pub(crate) struct Picked {
    /// A `session/set_config_option` went out: an options update the agent sent meanwhile is
    /// older than the options its last answer gave.
    sent: bool,
    /// The picks given to [`apply`].
    asked: Vec<Pick>,
    /// The picks the agent took.
    set: Vec<Pick>,
    /// The picks dropped.
    stale: Vec<Pick>,
}

/// Whether the pickers set options of `category`.
fn picked_category(category: ConfigCategory) -> bool {
    matches!(
        category,
        ConfigCategory::Model | ConfigCategory::ThoughtLevel
    )
}

/// The thread's header: the loaded stream's, else the one kept when it was unloaded.
fn header(actor: &Actor, thread: ThreadId) -> Option<&HeaderEntry> {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(actor.threads.get(&thread)?.header.as_ref())
}

/// The options the thread's header holds now (empty before any were logged).
pub(super) fn thread_options(actor: &Actor, thread: ThreadId) -> Vec<ConfigOption> {
    header(actor, thread).map_or_else(Vec::new, |header| header.config_options.clone())
}

/// The options the thread logged last; `None` until a session of it has attached. Before that,
/// logged options are the agent's for a session still being created, not the thread's own.
///
/// Known gap, as on main: an agent that reports its options only in an update before its
/// answer, with none in the answer, loses a new thread's create-time picks. Each is `Absent`
/// from the start's options, so none is set; the held update is released at the attach and
/// logged; from then on it is what this returns, and the picks are not tried again. Neither
/// claude-agent-acp nor codex-acp answers that way (test `an_answer_without_options_...`).
fn logged(actor: &Actor, thread: ThreadId) -> Option<&[ConfigOption]> {
    // Set by the attach commit alone (`session.rs`), and kept: a reopened thread has it.
    actor.threads.get(&thread)?.row.acp_session_id.as_ref()?;
    let options = &header(actor, thread)?.config_options;
    (!options.is_empty()).then_some(options.as_slice())
}

/// Refuses, with `command.invalid`, a `SetConfigOption` that is not a select value of a model
/// or effort option, as the thread's logged options (else its agent's kept ones) say. With
/// neither, before the agent has a session, it passes: the command then waits for the session,
/// whose attach checks it against the options the session started with ([`waited`]).
pub(super) fn settable(
    actor: &mut Actor,
    thread: ThreadId,
    option: &AgentChoiceId,
    value: &ConfigValue,
) -> Result<(), Error> {
    let picked = if let Some(options) = logged(actor, thread) {
        category_of(options, option)
    } else {
        let agent = actor.check_thread(thread)?.row.agent.clone();
        let attached = state(actor, thread)
            .and_then(|state| state.agent.as_deref())
            .is_some_and(|agent| agent.stage == Stage::Attached);
        let kept = actor.agent_options.of(agent.as_str());
        if kept.is_empty() && !attached {
            return Ok(());
        }
        category_of(kept, option)
    };
    verdict(option, picked, value)
}

/// A `SetConfigOption` that waited for the session goes to the agent only if `options`, the
/// ones the session started with, make it settable; otherwise its command is refused here.
pub(super) fn waited(
    actor: &mut Actor,
    options: &[ConfigOption],
    item: Waiting,
) -> Option<Waiting> {
    let Waiting::Config(ctx, option, value) = &item else {
        return Some(item);
    };
    match verdict(option, category_of(options, option), value) {
        Ok(()) => Some(item),
        Err(error) => {
            actor.finish_command(ctx.id, Err(error));
            None
        }
    }
}

/// Whether `options` hold `option` as a model or effort option; `None` without it.
fn category_of(options: &[ConfigOption], option: &AgentChoiceId) -> Option<bool> {
    options
        .iter()
        .find(|offered| offered.id == option.as_str())
        .map(|offered| picked_category(offered.category))
}

/// `settable`'s answer for an option found to be a model or effort one (`Some(true)`), another
/// (`Some(false)`) or none.
fn verdict(option: &AgentChoiceId, picked: Option<bool>, value: &ConfigValue) -> Result<(), Error> {
    let why = match (picked, value) {
        (Some(true), ConfigValue::Select(_)) => return Ok(()),
        (Some(true), ConfigValue::Boolean(_)) => "takes a choice, not on or off",
        (Some(false), _) => "is not the model or the effort, the only options set from Clauseo",
        (None, _) => "is not one the agent offers",
    };
    Err(Error::refused(
        ErrorCode::CommandInvalid,
        format!("config option {option} {why}"),
    ))
}

/// The picks `thread`'s new session takes, leaving out those `options` (its start's) already
/// hold: the thread's own logged values of the model and effort, else `CreateThread`'s config,
/// else the matter's prefs for its agent's kind; each with the value of a `SetConfigOption` of
/// its option waiting for the session instead, the last one waiting.
pub(super) fn wanted(actor: &mut Actor, thread: ThreadId, options: &[ConfigOption]) -> Vec<Pick> {
    candidates(actor, thread, options)
        .into_iter()
        .filter(|pick| offer(options, pick) != Offer::Held)
        .collect()
}

/// [`wanted`]'s picks before it leaves out those `options` hold already.
fn candidates(actor: &mut Actor, thread: ThreadId, options: &[ConfigOption]) -> Vec<Pick> {
    let mut picks: Vec<Pick> = match logged(actor, thread) {
        Some(own) => own.iter().filter_map(current_pick).collect(),
        None => remembered(actor, thread)
            .map(|picks| {
                let pairs = picks.as_map().iter();
                pairs
                    .map(|(id, value)| (id.clone(), value.clone()))
                    .collect()
            })
            .unwrap_or_default(),
    };
    for pick in waiting(actor, thread, options) {
        picks.retain(|(id, _)| *id != pick.0);
        picks.push(pick);
    }
    picks
}

/// The picks of the `SetConfigOption`s of a select value waiting for `thread`'s session, of the
/// model and effort options `options` hold: the last one waiting for each option.
fn waiting(actor: &mut Actor, thread: ThreadId, options: &[ConfigOption]) -> Vec<Pick> {
    let waiting = state(actor, thread)
        .and_then(|state| state.agent.as_deref())
        .map(|agent| agent.waiting.as_slice())
        .unwrap_or_default();
    let mut picks: Vec<Pick> = Vec::new();
    for item in waiting {
        if let Waiting::Config(_, option, ConfigValue::Select(value)) = item
            && category_of(options, option) == Some(true)
        {
            picks.retain(|(id, _)| id != option);
            picks.push((option.clone(), value.clone()));
        }
    }
    picks
}

/// The picks made while the session was prepared (after [`wanted`] was taken, so
/// waiting behind the first prompt): those `options`, the start's now, offer and do not hold,
/// and that `picked` did not try already. `session/prepare.rs` sets them in another round
/// before the attach, so the first prompt runs with them; a pick tried and kept (the
/// agent failed it otherwise) goes to the agent after the attach, as before.
pub(super) fn late(
    actor: &mut Actor,
    thread: ThreadId,
    options: &[ConfigOption],
    picked: &Picked,
) -> Vec<Pick> {
    let mut picks = waiting(actor, thread, options);
    picks.retain(|pick| offer(options, pick) == Offer::Offered && !picked.asked.contains(pick));
    picks
}

/// A logged model or effort option's current value, as a pick.
fn current_pick(option: &ConfigOption) -> Option<Pick> {
    let ConfigOptionKind::Select { current_value, .. } = &option.kind else {
        return None;
    };
    if !picked_category(option.category) {
        return None;
    }
    let id = AgentChoiceId::new(option.id.as_str()).ok()?;
    Some((id, AgentChoiceId::new(current_value.as_str()).ok()?))
}

/// `CreateThread`'s config, else the matter's prefs for the thread's agent kind.
fn remembered(actor: &Actor, thread: ThreadId) -> Option<&ConfigPicks> {
    let entry = actor.threads.get(&thread)?;
    if let Some(picks) = &entry.picks {
        return Some(picks);
    }
    let kind = AgentKind::of(&entry.row.agent)?;
    let row = &actor.matters.get(&entry.row.matter)?.row;
    row.agent_prefs.as_ref()?.of(kind)
}

/// Where a pick stands against the session's options.
#[derive(Debug, PartialEq, Eq)]
enum Offer {
    /// The option holds the value already.
    Held,
    /// The option offers the value.
    Offered,
    /// The option does not offer the value, or may not be set from Clauseo: the pick goes.
    Not,
    /// The session reports no option of that id: the pick waits for a session that does.
    Absent,
}

fn offer(options: &[ConfigOption], (id, value): &Pick) -> Offer {
    if !config_allowed(id.as_str(), Some(value.as_str())) {
        return Offer::Not;
    }
    let Some(option) = options.iter().find(|option| option.id == id.as_str()) else {
        return Offer::Absent;
    };
    let ConfigOptionKind::Select {
        current_value,
        options,
    } = &option.kind
    else {
        return Offer::Not;
    };
    if !picked_category(option.category) {
        return Offer::Not;
    }
    if current_value == value.as_str() {
        Offer::Held
    } else if options.iter().any(|choice| choice.value == value.as_str()) {
        Offer::Offered
    } else {
        Offer::Not
    }
}

/// Sets `picks` on `start`'s session, one at a time and the model first: each against the
/// options as the last answer gave them, which become `start`'s. A pick not offered, or one the
/// agent refuses as invalid params (-32602), is stale; one of an option the session lacks, or
/// that the agent answers with another error, is kept; a call that fails otherwise (the
/// connection closed) is logged and the rest are left.
pub(super) async fn apply(
    client: &AcpClient,
    start: &mut SessionStart,
    mut picks: Vec<Pick>,
) -> Picked {
    let rank = |(id, _): &Pick| match start.config_options.iter().find(|o| o.id == id.as_str()) {
        Some(option) if option.category == ConfigCategory::Model => 0,
        Some(option) if option.category == ConfigCategory::ThoughtLevel => 1,
        Some(_) | None => 2,
    };
    picks.sort_by_key(rank);
    let mut picked = Picked {
        asked: picks.clone(),
        ..Picked::default()
    };
    for pick in picks {
        match offer(&start.config_options, &pick) {
            Offer::Held | Offer::Absent => continue,
            Offer::Not => {
                picked.stale.push(pick);
                continue;
            }
            Offer::Offered => {}
        }
        picked.sent = true;
        let value = ConfigValue::Select(pick.1.clone());
        match client
            .set_config_option(&start.session, pick.0.clone(), value)
            .await
        {
            Ok(options) => {
                start.config_options = options;
                picked.set.push(pick);
            }
            Err(clauseo_acp::Error::Rpc {
                code: INVALID_PARAMS,
                ..
            }) => picked.stale.push(pick),
            Err(clauseo_acp::Error::Rpc { code, .. }) => {
                let value = pick_value(&start.config_options, &pick);
                tracing::warn!(option = %pick.0, value, code, "a config pick failed; kept");
            }
            Err(error) => {
                tracing::warn!(option = %pick.0, error = %Cause(&error), "a config pick was not set");
                break;
            }
        }
    }
    picked
}

/// The start's picks were set: host.log says which (the agent id, the model and the effort, as
/// `options`, the start's, file them), the options updates held meanwhile go (the start's
/// options are newer), and the stale picks are logged the same way and leave `CreateThread`'s
/// config and the matter's prefs, whose new row is written in a commit of its own.
pub(super) fn after(
    actor: &mut Actor,
    thread: ThreadId,
    picked: &Picked,
    options: &[ConfigOption],
) {
    let agent = actor
        .threads
        .get(&thread)
        .map_or_else(String::new, |entry| entry.row.agent.as_str().to_owned());
    if !picked.set.is_empty() {
        let held: Vec<Pick> = candidates(actor, thread, options)
            .into_iter()
            .filter(|pick| offer(options, pick) == Offer::Held && !picked.set.contains(pick))
            .collect();
        let (model, effort) = model_and_effort(options, &picked.set, &held);
        tracing::info!(target: LIFECYCLE, %agent, %model, %effort, "picks sent before first prompt");
    }
    if picked.sent {
        drop_held(actor, thread, "a pick was set since");
    }
    answer_waiting(actor, thread, &picked.set);
    if picked.stale.is_empty() {
        return;
    }
    let (model, effort) = model_and_effort(options, &picked.stale, &[]);
    tracing::info!(target: LIFECYCLE, %agent, %model, %effort, "the agent does not take a config pick; dropped");
    let drop_stale = |picks: &mut ConfigPicks| {
        let mut changed = false;
        for (option, value) in &picked.stale {
            if picks.as_map().get(option) == Some(value) {
                changed |= picks.remove(option.as_str());
            }
        }
        changed
    };
    if let Some(picks) = actor
        .threads
        .get_mut(&thread)
        .and_then(|entry| entry.picks.as_mut())
    {
        drop_stale(picks);
    }
    let Some(row) = prefs_row(actor, thread, drop_stale) else {
        return;
    };
    let written = Out::new(actor, thread, None).and_then(|mut out| {
        out.draft.matter_row(row);
        out.send(actor, Then::Nothing)
    });
    if let Err(error) = written {
        tracing::warn!(%thread, error = %Cause(&error), "the matter's prefs were not written");
    }
}

/// The `SetConfigOption`s of a select value waiting for `thread`'s session that the start's
/// `set` picks cover are answered: for each option set, the last one waiting with the value
/// set, and those of the option before it, which it overrode. One of the option after it, a
/// pick made while that value was being set, keeps waiting ([`late`]). Each is kept in the
/// matter's prefs as an answered one is (`answers::on_config`), in order, so the last stays.
fn answer_waiting(actor: &mut Actor, thread: ThreadId, set: &[Pick]) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    let waiting = std::mem::take(&mut agent.waiting);
    let config = |item: &Waiting| match item {
        Waiting::Config(_, option, ConfigValue::Select(value)) => {
            Some((option.clone(), value.clone()))
        }
        Waiting::Config(..) | Waiting::Turn(_) | Waiting::Mode(..) => None,
    };
    // For each option set, the last place it waits with the value set.
    let upto: Vec<(AgentChoiceId, usize)> = set
        .iter()
        .filter_map(|pick| {
            let at = waiting
                .iter()
                .rposition(|item| config(item).as_ref() == Some(pick))?;
            Some((pick.0.clone(), at))
        })
        .collect();
    let (answered, rest): (Vec<_>, Vec<_>) =
        waiting.into_iter().enumerate().partition(|(at, item)| {
            config(item).is_some_and(|(option, _)| {
                upto.iter().any(|(id, last)| *id == option && at <= last)
            })
        });
    agent.waiting = rest.into_iter().map(|(_, item)| item).collect();
    for (_, item) in answered {
        let Waiting::Config(ctx, option, value) = item else {
            continue;
        };
        let done = Out::new(actor, thread, Some(ctx.id)).and_then(|mut out| {
            if let Some(row) = remember(actor, thread, &(option, value)) {
                out.draft.matter_row(row);
            }
            super::answers::send_answer(actor, ctx, thread, out, Then::Nothing)
        });
        if let Err(error) = done {
            actor.finish_command(ctx.id, Err(error));
        }
    }
}

/// Whether `event` is an options update.
fn options_update(event: &AcpEvent) -> bool {
    matches!(
        event,
        AcpEvent::Update {
            update: AgentUpdate::ConfigOptions(_),
            ..
        }
    )
}

/// Drops the options updates held for `thread`'s session until its attach: the options of the
/// agent's answer that follows them are newer. Called when a session start's answer brings
/// options, and after a `session/set_config_option` went out before the attach (`why`, for
/// the log).
pub(super) fn drop_held(actor: &mut Actor, thread: ThreadId, why: &'static str) {
    let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut()) else {
        return;
    };
    let before = agent.replayed.len();
    agent.replayed.retain(|event| !options_update(event));
    let dropped = before - agent.replayed.len();
    if dropped > 0 {
        tracing::info!(target: LIFECYCLE, %thread, dropped, why, "held config options updates dropped");
    }
}

/// The held updates of `thread`'s session are passed on at its attach: logs how many of them
/// are options updates, which the thread then logs after the start's options.
pub(super) fn released(thread: ThreadId, held: &[AcpEvent]) {
    let released = held.iter().filter(|event| options_update(event)).count();
    if released > 0 {
        tracing::info!(target: LIFECYCLE, %thread, released, "held config options updates released");
    }
}

/// A `SetConfigOption` of `option` to `value` succeeded: the thread's matter row with the pick
/// kept for its agent kind, to write with the answer; `None` when the prefs hold it already, or
/// the agent has no kind.
pub(super) fn remember(
    actor: &mut Actor,
    thread: ThreadId,
    (option, value): &(AgentChoiceId, ConfigValue),
) -> Option<MatterRow> {
    let ConfigValue::Select(value) = value else {
        return None;
    };
    prefs_row(actor, thread, |picks| {
        picks.as_map().get(option) != Some(value)
            && picks.set(option.clone(), value.clone()).is_ok()
    })
}

/// The thread's matter row with its agent kind's picks changed by `change` (true when it
/// changed them), set on the host's row now; `None` when nothing changed or the agent has no
/// kind. A kind left with no pick goes, and prefs left with no kind are `None`.
fn prefs_row(
    actor: &mut Actor,
    thread: ThreadId,
    change: impl FnOnce(&mut ConfigPicks) -> bool,
) -> Option<MatterRow> {
    let entry = actor.threads.get(&thread)?;
    let kind = AgentKind::of(&entry.row.agent)?;
    let matter = actor.matters.get_mut(&entry.row.matter)?;
    let mut prefs = matter.row.agent_prefs.clone().unwrap_or_default();
    let picks = prefs.0.entry(kind).or_default();
    if !change(picks) {
        return None;
    }
    if picks.is_empty() {
        prefs.0.remove(&kind);
    }
    matter.row.agent_prefs = (!prefs.0.is_empty()).then_some(prefs);
    Some(matter.row.clone())
}

mod log;
#[cfg(test)]
mod tests;
