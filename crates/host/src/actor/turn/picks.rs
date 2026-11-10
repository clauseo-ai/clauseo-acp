//! The model and effort a thread's session runs with, as the pickers set them.
//!
//! - Only a select option whose category is `Model` or `ThoughtLevel` may be set ([`settable`]),
//!   as the thread's logged options say, else as its agent's kept ones (`agent_options.rs`);
//!   one that waits for a session is checked again against the options it starts with.
//!   Modes keep their own commands and rules, and every other option (Claude's `fast`) stays
//!   the agent's.
//! - Each session start (`session/new`, `session/load` or `session/resume`) takes [`wanted`]
//!   before its attach, so before any prompt: the thread's own values once it has logged
//!   options, since a reopened conversation keeps its model and effort and neither adapter
//!   restores both (claude-agent-acp 0.89.1 takes the effort from its settings, codex-acp 2.2.2
//!   both from Codex's resume answer); else `CreateThread`'s config; else the matter's prefs for
//!   the agent's kind; else nothing, the agent's defaults. [`apply`] sets each value that differs
//!   from the session's, the model first since the efforts offered follow it.
//! - A pick the agent does not offer, or refuses, is dropped ([`after`]): logged at info with
//!   its option and value ids, taken out of the matter's prefs, and the session goes on with
//!   the agent's value. A pick never fails or holds the launch, nor the prompt behind it.
//! - A `SetConfigOption` that succeeds keeps its pick in the matter's prefs for the thread's
//!   agent kind ([`remember`]).

use clauseo_acp::{AcpClient, AcpEvent, AgentUpdate, SessionStart};
use clauseo_protocol::{
    AgentChoiceId, AgentKind, ConfigCategory, ConfigOption, ConfigOptionKind, ConfigPicks,
    ConfigValue, ErrorCode, HeaderEntry, StreamId, ThreadId,
};
use clauseo_store::MatterRow;

use super::super::Actor;
use super::agents::{Stage, Waiting};
use super::out::Out;
use super::{Then, state};
use crate::error::Error;
use crate::log_file::Cause;

/// An option's id and the id of the value to set.
pub(super) type Pick = (AgentChoiceId, AgentChoiceId);

/// What [`apply`] did, for [`after`].
#[derive(Debug, Default)]
pub(crate) struct Picked {
    /// A `session/set_config_option` went out: an options update the agent sent meanwhile is
    /// older than the options its last answer gave.
    sent: bool,
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

/// The options the thread logged last; `None` before its first session's.
fn logged(actor: &Actor, thread: ThreadId) -> Option<&[ConfigOption]> {
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
/// else the matter's prefs for its agent's kind.
pub(super) fn wanted(actor: &Actor, thread: ThreadId, options: &[ConfigOption]) -> Vec<Pick> {
    let picks: Vec<Pick> = match logged(actor, thread) {
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
    picks
        .into_iter()
        .filter(|pick| offer(options, pick) != Offer::Held)
        .collect()
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
    /// No model or effort option of that id offers it.
    Not,
}

fn offer(options: &[ConfigOption], (id, value): &Pick) -> Offer {
    let option = options
        .iter()
        .find(|option| option.id == id.as_str() && picked_category(option.category));
    let Some(ConfigOptionKind::Select {
        current_value,
        options,
    }) = option.map(|option| &option.kind)
    else {
        return Offer::Not;
    };
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
/// agent answers with an error, is stale; a call that fails otherwise (the connection closed) is
/// logged and the rest are left.
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
    let mut picked = Picked::default();
    for pick in picks {
        match offer(&start.config_options, &pick) {
            Offer::Held => continue,
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
            Ok(options) => start.config_options = options,
            Err(clauseo_acp::Error::Rpc { .. }) => picked.stale.push(pick),
            Err(error) => {
                tracing::warn!(option = %pick.0, error = %Cause(&error), "a config pick was not set");
                break;
            }
        }
    }
    picked
}

/// The start's picks were set: the options updates held meanwhile go (the start's options are
/// newer), and each stale pick is logged and leaves `CreateThread`'s config and the matter's
/// prefs, whose new row is written in a commit of its own.
pub(super) fn after(actor: &mut Actor, thread: ThreadId, picked: &Picked) {
    if picked.sent
        && let Some(agent) = state(actor, thread).and_then(|state| state.agent.as_deref_mut())
    {
        agent.replayed.retain(|event| {
            !matches!(
                event,
                AcpEvent::Update {
                    update: AgentUpdate::ConfigOptions(_),
                    ..
                }
            )
        });
    }
    if picked.stale.is_empty() {
        return;
    }
    for (option, value) in &picked.stale {
        tracing::info!(%thread, %option, %value, "the agent does not take a config pick; dropped");
    }
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
