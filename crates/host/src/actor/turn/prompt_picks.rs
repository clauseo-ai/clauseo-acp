//! A prompt's own model and effort (`SendPrompt::config`), and the model and effort each turn
//! runs with (`TurnStarted::model` and `effort`).
//!
//! - The picks belong to the turn the `SendPrompt` makes: [`send_prompt`] holds them while the
//!   command is routed, and the step that mints the turn (`steps::accept`, `queued::log_queued`)
//!   takes them for it ([`claim`]). A steer that joins the running turn makes none, so its picks
//!   go. They are in memory only: a turn queued across a restart runs without them.
//! - While the thread has no session yet, they also become its `CreateThread` config, so its
//!   first session starts with them (`picks.rs`).
//! - At step 5, just before its prompt, the session gets each pick the thread's options offer
//!   and do not hold, the model first ([`for_prompt`], `prompt.rs`); the agent's last answer is
//!   logged as `ConfigOptionsReplaced` ([`on_set`]). One the agent refuses is skipped; the prompt
//!   goes out whatever happens.
//! - `TurnStarted` names the model and effort from the thread's options with the turn's picks
//!   over them ([`started_with`]); after a model pick without an effort pick, the effort is the
//!   current one when the new model takes it, else that model's default, as the agent's probe or
//!   latest session gave its efforts (`AgentSummary::models`).

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentChoiceId, AgentModels, ConfigCategory, ConfigOption, ConfigOptionKind,
    ConfigOptionsReplaced, ConfigPicks, NamedChoice, ThreadId, TurnId,
};

use super::super::Actor;
use super::out::Out;
use super::picks::Pick;
use super::{Then, state};
use crate::log_file::Cause;

/// A thread's prompt picks: those of the `SendPrompt` being routed, and each turn's.
#[derive(Debug, Default)]
pub(crate) struct PromptPicks {
    next: Option<ConfigPicks>,
    by_turn: BTreeMap<TurnId, ConfigPicks>,
}

/// Routes a `SendPrompt` with `config` held for the turn it makes.
pub(super) fn send_prompt<T>(
    actor: &mut Actor,
    thread: ThreadId,
    config: Option<ConfigPicks>,
    route: impl FnOnce(&mut Actor) -> T,
) -> T {
    let config = config.filter(|config| !config.is_empty());
    if let Some(config) = &config {
        before_session(actor, thread, config);
    }
    if let Some(state) = state(actor, thread) {
        state.prompt_picks.next = config;
    }
    let routed = route(actor);
    if let Some(state) = state(actor, thread) {
        state.prompt_picks.next = None;
    }
    routed
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

/// `turn` was minted for the `SendPrompt` being routed: it takes that command's picks.
pub(super) fn claim(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    if let Some(state) = state(actor, thread)
        && let Some(picks) = state.prompt_picks.next.take()
    {
        state.prompt_picks.by_turn.insert(turn, picks);
    }
}

/// The picks of `turn`'s prompt that `options` (the thread's now) offer and do not hold, the
/// model first; they leave the turn.
pub(super) fn for_prompt(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    options: &[ConfigOption],
) -> Vec<Pick> {
    let Some(picks) =
        state(actor, thread).and_then(|state| state.prompt_picks.by_turn.remove(&turn))
    else {
        return Vec::new();
    };
    let mut wanted: Vec<(u8, Pick)> = picks
        .as_map()
        .iter()
        .filter_map(|(option, value)| {
            let (category, current, offered) = select(options, option.as_str())?;
            let rank = match category {
                ConfigCategory::Model => 0,
                ConfigCategory::ThoughtLevel => 1,
                ConfigCategory::Mode | ConfigCategory::Other | ConfigCategory::Unknown => {
                    return None;
                }
            };
            (current != value.as_str() && offered.iter().any(|(id, _)| id == value.as_str()))
                .then(|| (rank, (option.clone(), value.clone())))
        })
        .collect();
    wanted.sort_by_key(|(rank, _)| *rank);
    wanted.into_iter().map(|(_, pick)| pick).collect()
}

/// The prompt's picks were set: the agent's last answer, `options`, is logged.
pub(super) fn on_set(actor: &mut Actor, thread: ThreadId, options: Vec<ConfigOption>) {
    let logged = Out::new(actor, thread, None).and_then(|mut out| {
        out.push(actor, ConfigOptionsReplaced { options })?;
        out.send(actor, Then::Nothing)
    });
    if let Err(error) = logged {
        tracing::warn!(%thread, error = %Cause(&error), "a prompt's picks were not logged");
    }
}

/// The model and effort `turn` runs with: the thread's options (`options`) with the turn's
/// picks over them (module doc).
pub(super) fn started_with(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    options: &[ConfigOption],
) -> (Option<NamedChoice>, Option<NamedChoice>) {
    let picks = state(actor, thread)
        .and_then(|state| state.prompt_picks.by_turn.get(&turn).cloned())
        .unwrap_or_default();
    let agent = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone());
    let models = agent.and_then(|agent| actor.agent_options.models_of(agent.as_str()));
    choose(options, &picks, models.as_ref())
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
