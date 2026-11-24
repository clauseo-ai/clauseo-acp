//! Code mode per turn: the header is in code mode from the running turn's first script call
//! (`clauseo_protocol::is_code_mode_call` on the call's ACP id) until the next turn starts; each
//! ended turn's step keeps its own mark from its `TurnEnded`; a fold of the log, and a snapshot,
//! rebuild both.

use clauseo_protocol::{DomainEvent, HeaderEntry, StepEntry, ToolCallStarted, TurnId, ViewEntry};
use clauseo_reducer::StreamState;

use crate::fixtures::{Log, start_turn, thread_created, tool_started, turn_ended};
use crate::helpers::{fold, round_trip};

#[cfg(test)]
fn header(state: &StreamState) -> &HeaderEntry {
    state.as_thread().unwrap().header().unwrap()
}

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

/// A tool call of `turn` whose ACP id is `acp_id`.
#[cfg(test)]
fn call(log: &mut Log, turn: TurnId, n: usize, acp_id: &str) -> DomainEvent {
    let id = log.fresh().unwrap();
    let DomainEvent::ToolCallStarted(mut started) = tool_started(turn, id, n).unwrap() else {
        panic!("not a tool call");
    };
    started.acp_id = acp_id.into();
    let started: Box<ToolCallStarted> = started;
    started.into()
}

/// Ends `turn` with `code_mode` as the host logs it.
#[cfg(test)]
fn end(log: &mut Log, turn: TurnId, code_mode: bool) {
    let checkpoint = log.fresh().unwrap();
    let DomainEvent::TurnEnded(mut ended) = turn_ended(turn, checkpoint).unwrap() else {
        panic!("not a turn end");
    };
    ended.code_mode = code_mode;
    log.push(ended).unwrap();
}

#[test]
fn a_script_call_marks_its_turn_and_the_next_turn_starts_unmarked() {
    let mut log = Log::thread(7).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let first = start_turn(&mut log, "Count the clauses.").unwrap();
    let plain = call(&mut log, first, 1, "toolu_01plain");
    log.push(plain).unwrap();
    assert!(
        !header(&fold(log.events())).code_mode,
        "a plain call is no script call"
    );
    let script = call(&mut log, first, 2, "toolu_cc_01script");
    log.push(script).unwrap();
    assert!(
        header(&fold(log.events())).code_mode,
        "the running turn is in code mode"
    );
    end(&mut log, first, true);
    let second = start_turn(&mut log, "Now summarise.").unwrap();
    assert!(
        !header(&fold(log.events())).code_mode,
        "a new turn starts unmarked"
    );
    end(&mut log, second, false);

    let state = fold(log.events());
    assert!(
        step(&state, first).code_mode,
        "the first answer keeps its mark"
    );
    assert!(
        !step(&state, second).code_mode,
        "the second answer has none"
    );
    // A reload (a snapshot read back) shows the same.
    let again = round_trip(&state);
    assert!(step(&again, first).code_mode);
    assert!(!step(&again, second).code_mode);
    assert_eq!(again, state);
}

#[test]
fn a_codex_repl_call_marks_its_turn() {
    let mut log = Log::thread(8).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Tabulate the dates.").unwrap();
    let script = call(&mut log, turn, 1, "repl_1");
    log.push(script).unwrap();
    assert!(header(&fold(log.events())).code_mode);
}
