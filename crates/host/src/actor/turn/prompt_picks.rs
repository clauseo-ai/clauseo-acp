//! A prompt's own model and effort (`SendPrompt::config`), and the model and effort each turn
//! runs with (`TurnStarted::model` and `effort`).
//!
//! - The picks belong to the turn the `SendPrompt` makes: [`send_prompt`] holds them while the
//!   command is routed, and the step that mints the turn (`steps::accept`, `queued::log_queued`)
//!   takes them for it ([`claim`]) and logs them on its `UserMessage` (`config`). A turn queued
//!   across a restart gets them back from its `UserMessage` (`queue/restore.rs`, [`restored`]),
//!   and so does a turn put back for the sign-in, whose new `UserMessage` logs them again
//!   (`queue/sign_in.rs`, from [`finished`]). The turn keeps them until it ends.
//! - A steer that joins the running answer cannot change its model: its picks go (`queue.rs`,
//!   [`drop_next`]), and never reach a starting session's `CreateThread` config either. A steer
//!   that becomes its own turn (it waits as the next turn, is sent now, or is held) keeps them.
//! - While the thread has no session yet, the picks a turn takes also become its `CreateThread`
//!   config, so its first session starts with them (`picks.rs`).
//! - At step 4, once the session is attached and before the checkpoint and `TurnStarted`, the
//!   session gets the model pick, then the effort pick checked against the model's answer
//!   (`send.rs`); the agent's last answer is logged as `ConfigOptionsReplaced`.
//! - `TurnStarted` names the model and effort that answer holds (else the thread's options,
//!   when nothing was set) ([`started_with`]): what the session really runs. A pick that could
//!   not be used (not offered, refused, or past the wait) gets the lawyer one notice in the turn,
//!   `NoticeKind::PickNotUsed`, naming what the answer runs with. Sets still out when the wait runs
//!   out finish in the background, and their answer is logged then.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentChoiceId, AgentModels, CompactString, ConfigCategory, ConfigOption, ConfigOptionKind,
    ConfigPicks, NamedChoice, Notice, ThreadId, TurnId,
};

use super::super::Actor;
use super::state;

mod send;

pub(super) use send::{Picked, on_picked, send_first};

/// A thread's prompt picks: those of the `SendPrompt` being routed, each turn's, and what
/// setting them did.
#[derive(Debug, Default)]
pub(crate) struct PromptPicks {
    next: Option<ConfigPicks>,
    /// Each turn's picks, from its `UserMessage` until it ends.
    by_turn: BTreeMap<TurnId, ConfigPicks>,
    /// What setting a turn's picks did, until its `TurnStarted` names it.
    ran: BTreeMap<TurnId, Ran>,
}

/// What setting a turn's picks did (`send.rs`).
#[derive(Debug, Default)]
struct Ran {
    /// The agent's last answer, when a set was taken: the options the turn runs with.
    answer: Option<Vec<ConfigOption>>,
    /// A pick could not be used.
    not_used: bool,
    /// While its sets are out and [`send::PICKS_WAIT`] has not run out: the wait's timer.
    waiting: Option<futures::future::AbortHandle>,
}

/// Routes a `SendPrompt` with `config` held for the turn it makes; a steer that joins the
/// running answer drops them first ([`drop_next`]).
pub(super) fn send_prompt<T>(
    actor: &mut Actor,
    thread: ThreadId,
    config: Option<ConfigPicks>,
    route: impl FnOnce(&mut Actor) -> T,
) -> T {
    let config = config.filter(|config| !config.is_empty());
    if let Some(state) = state(actor, thread) {
        state.prompt_picks.next = config;
    }
    let routed = route(actor);
    if let Some(state) = state(actor, thread) {
        state.prompt_picks.next = None;
    }
    routed
}

/// The `SendPrompt` being routed is a steer that joins the running answer, which keeps its
/// model: its picks go.
pub(super) fn drop_next(actor: &mut Actor, thread: ThreadId) {
    if let Some(state) = state(actor, thread) {
        state.prompt_picks.next = None;
    }
}

/// A thread with no session yet starts its first one with `config` over its `CreateThread`
/// config.
fn before_session(actor: &mut Actor, thread: ThreadId, config: &ConfigPicks) {
    let Some(entry) = actor.threads.get_mut(&thread) else {
        return;
    };
    if entry.row.acp_session_id.is_some() {
        return;
    }
    let mut picks = entry.picks.clone().unwrap_or_default();
    for (option, value) in config.as_map() {
        if picks.set(option.clone(), value.clone()).is_err() {
            return;
        }
    }
    entry.picks = Some(picks);
}

/// `turn` was minted for the `SendPrompt` being routed: it takes that command's picks (a thread
/// with no session yet starts its first one with them), and answers them as its
/// `UserMessage::config` logs them (empty for none), so a restart finds them again
/// ([`restored`]).
pub(super) fn claim(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
) -> BTreeMap<CompactString, CompactString> {
    let Some(picks) = state(actor, thread).and_then(|state| state.prompt_picks.next.take()) else {
        return BTreeMap::new();
    };
    let logged = picks
        .as_map()
        .iter()
        .map(|(option, value)| (option.as_str().into(), value.as_str().into()))
        .collect();
    before_session(actor, thread, &picks);
    if let Some(state) = state(actor, thread) {
        state.prompt_picks.by_turn.insert(turn, picks);
    }
    logged
}

/// A queued turn restored at start-up gets back the picks its `UserMessage` logged; a pick
/// whose id no command could name is left out.
pub(super) fn restored(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    logged: &BTreeMap<CompactString, CompactString>,
) {
    let map = logged
        .iter()
        .filter_map(|(option, value)| {
            let option = AgentChoiceId::new(option.as_str()).ok()?;
            Some((option, AgentChoiceId::new(value.as_str()).ok()?))
        })
        .collect();
    let Ok(picks) = ConfigPicks::new(map) else {
        return;
    };
    if !picks.is_empty()
        && let Some(state) = state(actor, thread)
    {
        state.prompt_picks.by_turn.insert(turn, picks);
    }
}

/// `turn` ends: its picks go, answered as a `UserMessage::config` logs them (empty for none),
/// for a copy of it put back in the queue (`sign_in.rs`).
pub(super) fn finished(
    picks: &mut PromptPicks,
    turn: TurnId,
) -> BTreeMap<CompactString, CompactString> {
    picks.ran.remove(&turn);
    picks
        .by_turn
        .remove(&turn)
        .map(|picks| {
            picks
                .as_map()
                .iter()
                .map(|(option, value)| (option.as_str().into(), value.as_str().into()))
                .collect()
        })
        .unwrap_or_default()
}

/// The model and effort `turn` runs with, and the notice owed when a pick of its could not be
/// used: those of the agent's last answer to its picks, else of the thread's options
/// (`options`), the session's own (module doc).
pub(super) fn started_with(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    options: Vec<ConfigOption>,
) -> (Option<NamedChoice>, Option<NamedChoice>, Option<Notice>) {
    let ran = state(actor, thread)
        .and_then(|state| state.prompt_picks.ran.remove(&turn))
        .unwrap_or_default();
    let options = ran.answer.unwrap_or(options);
    let agent = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone());
    let models = agent.and_then(|agent| actor.agent_options.models_of(agent.as_str()));
    let (model, effort) = choose(&options, &ConfigPicks::default(), models.as_ref());
    let notice = ran
        .not_used
        .then(|| send::not_used(model.as_ref(), effort.as_ref()));
    (model, effort, notice)
}

/// [`started_with`] over plain values.
pub(crate) fn choose(
    options: &[ConfigOption],
    picks: &ConfigPicks,
    models: Option<&AgentModels>,
) -> (Option<NamedChoice>, Option<NamedChoice>) {
    let of_category = |category: ConfigCategory| {
        options.iter().find(|option| {
            option.category == category && matches!(option.kind, ConfigOptionKind::Select { .. })
        })
    };
    let picked = |option: &ConfigOption| {
        let id = AgentChoiceId::new(option.id.as_str()).ok()?;
        picks
            .as_map()
            .get(&id)
            .map(|value| value.as_str().to_owned())
    };
    let Some(model_option) = of_category(ConfigCategory::Model) else {
        return (None, None);
    };
    let (_, current_model, model_choices) =
        select(options, &model_option.id).unwrap_or((ConfigCategory::Model, "", Vec::new()));
    let model_id = picked(model_option)
        .filter(|value| model_choices.iter().any(|(id, _)| id == value))
        .unwrap_or_else(|| current_model.to_owned());
    let model = named_of(&model_choices, &model_id);
    let switched = model_id != current_model;
    let effort_option = of_category(ConfigCategory::ThoughtLevel);
    let effort = effort_option.and_then(|option| {
        let (_, current, choices) = select(options, &option.id)?;
        let known = models.and_then(|models| models.model(&model_id));
        // The new model's levels, when known; else the option's, which are the old model's.
        let levels: Vec<(String, String)> = match (switched, known) {
            (true, Some(known)) => known
                .efforts
                .iter()
                .map(|effort| (effort.id.to_string(), effort.name.to_string()))
                .collect(),
            _ => choices,
        };
        let wanted = picked(option)
            .filter(|value| levels.iter().any(|(id, _)| id == value))
            .or_else(|| {
                levels
                    .iter()
                    .any(|(id, _)| id == current)
                    .then(|| current.to_owned())
            })
            .or_else(|| known?.default_effort.as_ref().map(ToString::to_string))?;
        named_of(&levels, &wanted)
    });
    (model, effort)
}

fn named_of(choices: &[(String, String)], id: &str) -> Option<NamedChoice> {
    let (id, name) = choices.iter().find(|(choice, _)| choice == id)?;
    Some(NamedChoice {
        id: id.as_str().into(),
        name: name.as_str().into(),
    })
}

/// A select option's category, current value and choices (id, name).
type Select<'a> = (ConfigCategory, &'a str, Vec<(String, String)>);

/// The category, current value and choices (id, name) of the select option `id` of `options`.
fn select<'a>(options: &'a [ConfigOption], id: &str) -> Option<Select<'a>> {
    let option = options.iter().find(|option| option.id == id)?;
    let ConfigOptionKind::Select {
        current_value,
        options: choices,
    } = &option.kind
    else {
        return None;
    };
    let choices = choices
        .iter()
        .map(|choice| (choice.value.to_string(), choice.name.to_string()))
        .collect();
    Some((option.category, current_value.as_str(), choices))
}

#[cfg(test)]
mod tests;
