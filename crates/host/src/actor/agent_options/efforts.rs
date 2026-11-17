//! Which models take an effort: `ConfigChoice::takes_effort` on each choice of a `Model`
//! option, so the app can hide its effort picker for a model that has none.
//!
//! No adapter reports it per model. claude-agent-acp 0.89.1 lists its models with a value, a
//! name and a description only (`session-model.js:192-206`), and sends its `thought_level`
//! option only while the current model has effort levels (`buildEffortConfigOption`,
//! `session-effort.js:61-67`, called from `buildConfigOptions`, `acp-agent.js:8260-8262`, and
//! rebuilt on each model switch, `acp-agent.js:6796-6805`). So a report says it for its current
//! model alone: true when the report holds an option of the `ThoughtLevel` category, false when
//! it holds none. Every other model keeps what an earlier report of the thread or of its agent
//! said (a false there stays false), and is true when none said: unknown reads as true, as
//! before the field.

use clauseo_protocol::{ConfigCategory, ConfigOption, ConfigOptionKind, StreamId, ThreadId};

use super::super::Actor;

/// Marks the model choices of `options`, a report of `thread`'s agent about to be logged, with
/// the thread's logged options and its agent's kept ones as the earlier reports.
pub(crate) fn mark_for(actor: &mut Actor, thread: ThreadId, options: &mut [ConfigOption]) {
    if !options.iter().any(is_model) {
        return;
    }
    let logged = actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or_else(|| actor.threads.get(&thread)?.header.as_ref())
        .map(|header| header.config_options.clone())
        .unwrap_or_default();
    let agent = actor
        .threads
        .get(&thread)
        .map(|entry| entry.row.agent.clone());
    let kept = agent
        .map(|agent| actor.agent_options.of(agent.as_str()).to_vec())
        .unwrap_or_default();
    mark(options, &[&logged, &kept]);
}

/// Marks each choice of `options`' `Model` select options: the current one by whether
/// `options` holds a `ThoughtLevel` option, any other false when one of `earlier` marks it
/// false, else true.
pub(crate) fn mark(options: &mut [ConfigOption], earlier: &[&[ConfigOption]]) {
    let takes = options
        .iter()
        .any(|option| option.category == ConfigCategory::ThoughtLevel);
    for option in options.iter_mut().filter(|option| is_model(option)) {
        let ConfigOptionKind::Select {
            current_value,
            options: choices,
        } = &mut option.kind
        else {
            continue;
        };
        for choice in choices {
            choice.takes_effort = if choice.value == *current_value {
                takes
            } else {
                !earlier
                    .iter()
                    .any(|report| flag(report, &choice.value) == Some(false))
            };
        }
    }
}

/// Gives each model choice of `kept` the mark of the same model in `reported`, where it has
/// one: the kept options hold the values a session started with, which may predate the marks.
pub(crate) fn carry(kept: &mut [ConfigOption], reported: &[ConfigOption]) {
    for option in kept.iter_mut().filter(|option| is_model(option)) {
        if let ConfigOptionKind::Select { options, .. } = &mut option.kind {
            for choice in options {
                if let Some(takes) = flag(reported, &choice.value) {
                    choice.takes_effort = takes;
                }
            }
        }
    }
}

/// The mark of the model `value` among `options`' model choices, if they list it.
fn flag(options: &[ConfigOption], value: &str) -> Option<bool> {
    options
        .iter()
        .filter(|option| is_model(option))
        .find_map(|option| {
            let ConfigOptionKind::Select { options, .. } = &option.kind else {
                return None;
            };
            options
                .iter()
                .find(|choice| choice.value == value)
                .map(|choice| choice.takes_effort)
        })
}

/// Whether `option` picks the model.
fn is_model(option: &ConfigOption) -> bool {
    option.category == ConfigCategory::Model
}

#[cfg(test)]
mod tests;
