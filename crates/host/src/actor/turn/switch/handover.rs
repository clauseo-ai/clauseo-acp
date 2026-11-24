//! The handover a switch gives the new agent, read from the thread's whole log: the items, the
//! budget and the text (`crate::handover`), and the two ends as the thread shows them.

use std::collections::BTreeMap;

use clauseo_protocol::{
    AgentId, AgentKind, ConfigCategory, ConfigOption, ConfigOptionKind, ContentBlock, Count,
    DomainEvent, Endpoint, Envelope, HeaderEntry, NamedChoice, StreamId, ThreadId, TurnId,
};

use super::super::super::Actor;
use crate::handover::{self, Item};

/// The name the handover gives an agent: Claude, `ChatGPT`, or another agent's registry id.
pub(super) fn display_name(agent: &AgentId) -> String {
    match AgentKind::of(agent) {
        Some(AgentKind::Claude) => "Claude".to_owned(),
        Some(AgentKind::Codex) => "ChatGPT".to_owned(),
        None => agent.as_str().to_owned(),
    }
}

/// What a switch hands over, and the counts its `ContextHandoff` logs.
#[derive(Debug)]
pub(super) struct Built {
    pub(super) fit: Fit,
    pub(super) answers: Count,
    pub(super) included: Count,
    pub(super) left_out: Count,
}

/// The handover's text, if any.
#[derive(Debug)]
pub(super) enum Fit {
    /// No earlier finished turn: nothing to hand over, and no `ContextHandoff`.
    Nothing,
    /// The text: the items that fit, or the header alone saying they are all left out.
    Text(String),
    /// Not even the header fits.
    TooLong,
}

/// The handover of `log`, the thread's whole log, from `from` to a new agent, for a prompt of
/// `prompt`. The new agent's context size is not known before its session reports it, so the
/// budget assumes `handover`'s default window; the new agent has no history of its own.
pub(super) fn build(log: &[Envelope], from: &AgentId, prompt: &[ContentBlock]) -> Built {
    let names = answered_by(log, from);
    let fallback = display_name(from);
    let by = |turn: TurnId| {
        names
            .get(&turn)
            .cloned()
            .unwrap_or_else(|| fallback.clone())
    };
    let items = handover::items_from_log(log, None, &by);
    let request = prompt_bytes(prompt);
    let budget = handover::budget(None, 0, request);
    let answers = items
        .iter()
        .filter(|item| matches!(item, Item::Answer { .. }))
        .count();
    let (fit, included, left_out) = match handover::build(&items, &fallback, budget) {
        Some(built) => (Fit::Text(built.text), built.included, built.left_out),
        None if items.is_empty() => (Fit::Nothing, 0, 0),
        None => (Fit::TooLong, 0, items.len()),
    };
    Built {
        fit,
        answers: count(answers),
        included: count(included),
        left_out: count(left_out),
    }
}

/// The name of the agent that answered each turn that started: the thread's first agent, then
/// each `AgentSwitched`'s new one.
fn answered_by(log: &[Envelope], first: &AgentId) -> BTreeMap<TurnId, String> {
    let mut agent = first.clone();
    let mut names = BTreeMap::new();
    for envelope in log {
        if let DomainEvent::ThreadCreated(created) = &envelope.event {
            agent.clone_from(&created.agent);
        } else if let DomainEvent::AgentSwitched(switched) = &envelope.event {
            agent.clone_from(&switched.to);
        } else if let DomainEvent::TurnStarted(started) = &envelope.event {
            names.insert(started.turn, display_name(&agent));
        }
    }
    names
}

/// The prompt's size as the budget counts it: the bytes of its text and its files' names.
fn prompt_bytes(prompt: &[ContentBlock]) -> u64 {
    let bytes: usize = prompt
        .iter()
        .map(|block| match block {
            ContentBlock::Text { text } => text.len(),
            ContentBlock::ResourceLink { name, .. } => name.len(),
            ContentBlock::Image { .. }
            | ContentBlock::Audio { .. }
            | ContentBlock::Resource { .. }
            | ContentBlock::Unsupported { .. } => 0,
        })
        .sum();
    u64::try_from(bytes).unwrap_or(u64::MAX)
}

fn count(n: usize) -> Count {
    Count::clamped(u64::try_from(n).unwrap_or(u64::MAX))
}

/// `agent` as the thread shows it now: the model and effort from its logged options when it has
/// them. The agent a switch goes to shows neither: its session has not reported yet.
pub(super) fn endpoint(actor: &Actor, thread: ThreadId, agent: &AgentId, shown: bool) -> Endpoint {
    let options = shown
        .then(|| header(actor, thread))
        .flatten()
        .map(|header| header.config_options.as_slice())
        .unwrap_or_default();
    Endpoint {
        agent: agent.clone(),
        model: current_choice(options, ConfigCategory::Model),
        effort: current_choice(options, ConfigCategory::ThoughtLevel),
    }
}

/// The thread's header: the loaded stream's, else the one kept when it was unloaded.
fn header(actor: &Actor, thread: ThreadId) -> Option<&HeaderEntry> {
    actor
        .loaded(StreamId::Thread(thread))
        .and_then(|loaded| loaded.state.as_thread())
        .and_then(|state| state.header())
        .or(actor.threads.get(&thread)?.header.as_ref())
}

/// The current value of the select option of `category`: its id and its shown name.
fn current_choice(options: &[ConfigOption], category: ConfigCategory) -> Option<NamedChoice> {
    let option = options.iter().find(|option| option.category == category)?;
    let ConfigOptionKind::Select {
        current_value,
        options,
    } = &option.kind
    else {
        return None;
    };
    options
        .iter()
        .find(|choice| choice.value == *current_value)
        .map(|choice| NamedChoice {
            id: choice.value.as_str().into(),
            name: choice.name.as_str().into(),
        })
}

/// The prompt the new agent gets: the handover, then the lawyer's own words after
/// "User message:", in the first text block, or in a block of its own before a file.
pub(super) fn wrap(text: &str, mut prompt: Vec<ContentBlock>) -> Vec<ContentBlock> {
    match prompt.first_mut() {
        Some(ContentBlock::Text { text: first }) => {
            *first = format!("{text}\n\nUser message:\n{first}");
        }
        _ => prompt.insert(
            0,
            ContentBlock::Text {
                text: format!("{text}\n\nUser message:\n"),
            },
        ),
    }
    prompt
}
