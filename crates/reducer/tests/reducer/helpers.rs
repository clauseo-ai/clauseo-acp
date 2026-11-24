//! A helper's events in the view (`thread/helpers.rs`): its start opens a `Helper` entry, each
//! run of its words is a `HelperText` entry, its steps are tool calls naming it, and its end
//! settles the entry once. Out of order: a step or words before the start fold and keep the
//! helper; an end twice, an end of an unknown helper and a start twice change nothing. A
//! snapshot and a fold from the log give the same view.

use clauseo_protocol::{
    DomainEvent, EntryId, HelperEnded, HelperEntry, HelperId, HelperOutcome, HelperStarted,
    HelperText, TextOrBlob, ToolCallId, TurnId, ViewEntry,
};
use clauseo_reducer::{Anomaly, StreamState, ViewChange, apply};

use crate::events::{short, tool_started, tool_status};
use crate::fixtures::{Log, start_turn, thread_created};
use crate::helpers::{fold, round_trip};

#[cfg(test)]
fn turn_log(n: u64) -> (Log, TurnId) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Read the lease.").unwrap();
    (log, turn)
}

#[cfg(test)]
fn started(turn: TurnId, helper: HelperId, call: Option<ToolCallId>) -> DomainEvent {
    Box::new(HelperStarted {
        turn,
        helper,
        acp_id: short("toolu_A"),
        parent: None,
        call,
        name: short("paper-reader"),
        label: Some(short("Read the lease")),
        task: Some(TextOrBlob::Inline("Read lease.docx.".to_owned())),
        model: Some(short("haiku")),
    })
    .into()
}

#[cfg(test)]
fn words(turn: TurnId, helper: HelperId, text: &str) -> DomainEvent {
    HelperText {
        turn,
        helper,
        text: text.to_owned(),
    }
    .into()
}

#[cfg(test)]
fn ended(helper: HelperId, outcome: HelperOutcome) -> DomainEvent {
    HelperEnded {
        helper,
        outcome,
        summary: Some(TextOrBlob::Inline("Two break clauses.".to_owned())),
    }
    .into()
}

/// A step of `helper`.
#[cfg(test)]
fn step(turn: TurnId, call: ToolCallId, helper: HelperId) -> DomainEvent {
    let DomainEvent::ToolCallStarted(mut started) = tool_started(turn, call, 2).unwrap() else {
        unreachable!("tool_started builds a ToolCallStarted");
    };
    started.helper = Some(helper);
    DomainEvent::ToolCallStarted(started)
}

/// How many helpers the state counts open.
#[cfg(test)]
fn open(state: &StreamState) -> u32 {
    state.as_thread().unwrap().open_helpers()
}

#[cfg(test)]
fn helper_entry(state: &StreamState, helper: HelperId) -> Option<HelperEntry> {
    state.entries().values().find_map(|entry| match entry {
        ViewEntry::Helper(found) if found.helper == helper => Some(found.as_ref().clone()),
        _ => None,
    })
}

/// The helper's own entries in order: `start`, `step`, `words <text>`.
#[cfg(test)]
fn shape(state: &StreamState, helper: HelperId) -> Vec<String> {
    state
        .entries()
        .values()
        .filter_map(|entry| match entry {
            ViewEntry::Helper(found) if found.helper == helper => Some("start".to_owned()),
            ViewEntry::ToolCall(call) if call.helper == Some(helper) => Some("step".to_owned()),
            ViewEntry::HelperText(text) if text.helper == helper => {
                Some(format!("words {}", text.text))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_helper_folds_into_its_card_its_steps_and_its_words() {
    let (mut log, turn) = turn_log(81);
    let launch: ToolCallId = log.fresh().unwrap();
    let read: ToolCallId = log.fresh().unwrap();
    let helper: HelperId = log.fresh().unwrap();
    log.push(tool_started(turn, launch, 1).unwrap()).unwrap();
    let start = log.push(started(turn, helper, Some(launch))).unwrap();
    log.push(step(turn, read, helper)).unwrap();
    log.push(words(turn, helper, "The lease runs ")).unwrap();
    log.push(words(turn, helper, "to 2031.")).unwrap();
    log.push(tool_status(
        read,
        clauseo_protocol::ToolCallStatus::Completed,
    ))
    .unwrap();
    let end = log.push(ended(helper, HelperOutcome::Done)).unwrap();
    let events = log.into_events();
    let state = fold(&events);
    let entry = helper_entry(&state, helper).unwrap();
    assert_eq!(entry.order, start.seq);
    assert_eq!(entry.call, Some(launch));
    assert_eq!(entry.name, "paper-reader");
    assert_eq!(entry.started_at, start.ts);
    assert_eq!(entry.outcome, Some(HelperOutcome::Done));
    assert_eq!(entry.finished_at, Some(end.ts));
    assert_eq!(
        entry.summary,
        Some(TextOrBlob::Inline("Two break clauses.".to_owned()))
    );
    assert_eq!(
        shape(&state, helper),
        ["start", "step", "words The lease runs ", "words to 2031."]
    );
    // The snapshot holds the same view, and a helper's end still settles once after it.
    let restored = round_trip(&state);
    assert_eq!(restored.entries(), state.entries());
}

#[test]
fn a_running_helper_has_no_outcome_and_its_end_upserts_it_once() {
    let (mut log, turn) = turn_log(82);
    let helper: HelperId = log.fresh().unwrap();
    log.push(started(turn, helper, None)).unwrap();
    let mut state = fold(log.events());
    assert_eq!(helper_entry(&state, helper).unwrap().outcome, None);
    assert_eq!(open(&state), 1);
    assert_eq!(open(&round_trip(&state)), 1, "a snapshot counts it again");
    let end = log.push(ended(helper, HelperOutcome::Failed)).unwrap();
    let applied = apply(&mut state, &end).unwrap();
    assert_eq!(applied.anomalies, Vec::<Anomaly>::new());
    assert_eq!(open(&state), 0);
    let [ViewChange::Upsert(entry)] = applied.changes.as_slice() else {
        panic!("{:?}", applied.changes);
    };
    let ViewEntry::Helper(entry) = entry.as_ref() else {
        panic!("{entry:?}");
    };
    assert_eq!(entry.outcome, Some(HelperOutcome::Failed));
}

#[test]
fn a_second_end_changes_nothing() {
    let (mut log, turn) = turn_log(83);
    let helper: HelperId = log.fresh().unwrap();
    log.push(started(turn, helper, None)).unwrap();
    log.push(ended(helper, HelperOutcome::Done)).unwrap();
    let mut state = fold(log.events());
    let again = log.push(ended(helper, HelperOutcome::Stopped)).unwrap();
    let applied = apply(&mut state, &again).unwrap();
    assert_eq!(applied.changes, Vec::<ViewChange>::new());
    assert_eq!(
        applied.anomalies,
        [Anomaly::AlreadySettled {
            event: "helper_ended",
            target: EntryId::Helper(helper),
        }]
    );
    assert_eq!(
        helper_entry(&state, helper).unwrap().outcome,
        Some(HelperOutcome::Done)
    );
    assert_eq!(open(&state), 0, "a second end counts nothing");
}

#[test]
fn an_end_of_an_unknown_helper_and_a_second_start_change_nothing() {
    let (mut log, turn) = turn_log(84);
    let helper: HelperId = log.fresh().unwrap();
    let stranger: HelperId = log.fresh().unwrap();
    log.push(started(turn, helper, None)).unwrap();
    let mut state = fold(log.events());
    let unknown = log.push(ended(stranger, HelperOutcome::Done)).unwrap();
    let applied = apply(&mut state, &unknown).unwrap();
    assert_eq!(applied.changes, Vec::<ViewChange>::new());
    assert_eq!(
        applied.anomalies,
        [Anomaly::UnknownReference {
            event: "helper_ended",
            target: EntryId::Helper(stranger),
        }]
    );
    let twice = log.push(started(turn, helper, None)).unwrap();
    let applied = apply(&mut state, &twice).unwrap();
    assert_eq!(applied.changes, Vec::<ViewChange>::new());
    assert_eq!(
        applied.anomalies,
        [Anomaly::Duplicate {
            event: "helper_started",
            target: EntryId::Helper(helper),
        }]
    );
}

/// A step and words that come before their helper's start fold as they are, naming the helper;
/// the start then opens the card after them, so a screen shows them flat until it has the card.
#[test]
fn a_step_and_words_before_the_start_keep_their_helper() {
    let (mut log, turn) = turn_log(85);
    let read: ToolCallId = log.fresh().unwrap();
    let helper: HelperId = log.fresh().unwrap();
    log.push(step(turn, read, helper)).unwrap();
    log.push(words(turn, helper, "Early.")).unwrap();
    let state = fold(log.events());
    assert_eq!(helper_entry(&state, helper), None);
    assert_eq!(shape(&state, helper), ["step", "words Early."]);
    log.push(started(turn, helper, None)).unwrap();
    let state = fold(log.events());
    assert_eq!(shape(&state, helper), ["step", "words Early.", "start"]);
}
