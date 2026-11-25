//! What each step ran with (`StepEntry::model`, `effort` and `agent`), for the app's "model
//! changed" divider: the model and effort of the turn's `TurnStarted`, and the thread's agent
//! when it started, so an answer after a switch names the agent that gave it. A step folded
//! before the fields, and a turn logged before `TurnStarted` named its model, have none.

use clauseo_protocol::{
    AgentId, AgentSwitched, NamedChoice, StepEntry, TurnId, TurnStarted, ViewEntry,
};
use clauseo_reducer::StreamState;

use crate::fixtures::{Log, start_turn, thread_created, turn_ended, user_message};
use crate::helpers::{fold, round_trip};

#[cfg(test)]
fn step(state: &StreamState, turn: TurnId) -> StepEntry {
    state
        .entries()
        .values()
        .find_map(|entry| match entry {
            ViewEntry::Step(step) if step.turn == turn => Some(step.clone()),
            _ => None,
        })
        .unwrap()
}

#[cfg(test)]
fn named(id: &str, name: &str) -> NamedChoice {
    NamedChoice {
        id: id.into(),
        name: name.into(),
    }
}

#[cfg(test)]
fn agent(id: &str) -> AgentId {
    id.parse().unwrap()
}

/// A turn of `prompt` started with `model` and `effort`, then ended.
#[cfg(test)]
fn turn_with(
    log: &mut Log,
    prompt: &str,
    ran: (Option<NamedChoice>, Option<NamedChoice>),
) -> TurnId {
    let turn: TurnId = log.fresh().unwrap();
    let message = log.fresh().unwrap();
    log.push(user_message(turn, message, prompt)).unwrap();
    let checkpoint_before = log.fresh().unwrap();
    log.push(TurnStarted {
        turn,
        checkpoint_before,
        model: ran.0,
        effort: ran.1,
    })
    .unwrap();
    let checkpoint = log.fresh().unwrap();
    log.push(turn_ended(turn, checkpoint).unwrap()).unwrap();
    turn
}

#[test]
fn a_step_carries_the_model_and_effort_of_its_turn_started() {
    let mut log = Log::thread(3).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let opus = (Some(named("opus", "Opus 5.5")), Some(named("high", "High")));
    let first = turn_with(&mut log, "Read the lease.", opus);
    let sonnet = (Some(named("sonnet", "Sonnet 5.5")), None);
    let second = turn_with(&mut log, "And clause 5?", sonnet);
    let state = fold(log.events());
    let (one, two) = (step(&state, first), step(&state, second));
    assert_eq!(one.model, Some(named("opus", "Opus 5.5")));
    assert_eq!(one.effort, Some(named("high", "High")));
    assert_eq!(one.agent, Some(agent("claude-acp")));
    assert_eq!(two.model, Some(named("sonnet", "Sonnet 5.5")));
    assert_eq!(two.effort, None, "not kept from the turn before");
    assert_eq!(round_trip(&state), state);
}

#[test]
fn an_answer_after_a_switch_names_the_new_agent() {
    let mut log = Log::thread(4).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let opus = (Some(named("opus", "Opus 5.5")), Some(named("high", "High")));
    let before = turn_with(&mut log, "Read the lease.", opus);
    log.push(AgentSwitched {
        from: agent("claude-acp"),
        to: agent("codex-acp"),
    })
    .unwrap();
    let gpt = (
        Some(named("gpt-5.5", "GPT-5.5")),
        Some(named("medium", "Medium")),
    );
    let after = turn_with(&mut log, "ChatGPT, review", gpt);
    let state = fold(log.events());
    assert_eq!(step(&state, before).agent, Some(agent("claude-acp")));
    let answer = step(&state, after);
    assert_eq!(answer.agent, Some(agent("codex-acp")));
    assert_eq!(answer.model, Some(named("gpt-5.5", "GPT-5.5")));
    assert_eq!(answer.effort, Some(named("medium", "Medium")));
    assert_eq!(round_trip(&state), state);
}

#[test]
fn an_old_log_has_none() {
    // A turn logged before `TurnStarted` named its model and effort.
    let mut log = Log::thread(5).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Read the lease.").unwrap();
    let checkpoint = log.fresh().unwrap();
    log.push(turn_ended(turn, checkpoint).unwrap()).unwrap();
    let card = step(&fold(log.events()), turn);
    assert_eq!((card.model.clone(), card.effort.clone()), (None, None));
    // The fields are left out when `None`, and a step written before them reads as none.
    let mut json = serde_json::to_value(&card).unwrap();
    let fields = json.as_object_mut().unwrap();
    assert!(!fields.contains_key("model") && !fields.contains_key("effort"));
    fields.remove("agent");
    let old: StepEntry = serde_json::from_value(json).unwrap();
    assert_eq!((old.model, old.effort, old.agent), (None, None, None));
}
