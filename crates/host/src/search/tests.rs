//! The fold against the reducer, and the generations, without a store.

use clauseo_protocol::{
    AgentId, CheckpointId, ContentBlock, Delivery, DomainEvent, Envelope, EventId, MessageId,
    QueuedPromptEdited, Seq, StepUndone, StopReason, StreamId, TextAppended, TextKind, ThreadId,
    ToolCallId, ToolCallPatch, ToolCallStarted, ToolCallStatus, ToolCallUpdated, ToolKind,
    TurnEnded, TurnId, TurnStarted, UserMessage, ViewEntry,
};
use clauseo_reducer::{StreamState, ViewChange};
use clauseo_search::EntryKind;

use super::Generations;
use super::fold::{ThreadMemo, Write};
use crate::pipeline::tests::support::{id, ts};

fn thread() -> ThreadId {
    id(1)
}

fn turn() -> TurnId {
    id(2)
}

fn seq(n: u64) -> Seq {
    Seq::new(n).unwrap()
}

/// Envelopes of the test thread from seq 1, one per event.
fn envelopes(events: Vec<DomainEvent>) -> Vec<Envelope> {
    events
        .into_iter()
        .zip(1_u64..)
        .map(|(event, n)| {
            let stream = StreamId::Thread(thread());
            Envelope::new(id::<EventId>(1_000 + n), stream, seq(n), ts(0), None, event)
        })
        .collect()
}

fn text(message: u64, kind: TextKind, chunk: &str) -> DomainEvent {
    DomainEvent::TextAppended(TextAppended {
        turn: turn(),
        message: id::<MessageId>(message),
        acp_message_id: None,
        kind,
        text: chunk.to_owned(),
    })
}

fn prompt(content: &str) -> Vec<ContentBlock> {
    vec![ContentBlock::Text {
        text: content.to_owned(),
    }]
}

/// A turn: a prompt, a thought, an answer in three chunks, a tool call renamed, a second
/// answer, then the end. Seqs: 1 created, 2 prompt, 3 started, 4 thought, 5-7 answer,
/// 8 tool, 9 rename, 10 answer, 11 end.
fn turn_events() -> Vec<DomainEvent> {
    vec![
        DomainEvent::ThreadCreated(clauseo_protocol::ThreadCreated {
            matter: id(9),
            agent: AgentId::new("fake-agent").unwrap(),
            title: Some("Lease".into()),
        }),
        DomainEvent::UserMessage(UserMessage {
            turn: turn(),
            message: id::<MessageId>(20),
            acp_message_id: None,
            content: prompt("is clause 4 valid?"),
            delivery: Delivery::Queue,
        }),
        DomainEvent::TurnStarted(TurnStarted {
            turn: turn(),
            checkpoint_before: id::<CheckpointId>(30),
        }),
        text(21, TextKind::Thought, "thinking about clause 4"),
        text(22, TextKind::Answer, "Clause 4 "),
        text(22, TextKind::Answer, "is void "),
        text(22, TextKind::Answer, "under s. 23."),
        DomainEvent::ToolCallStarted(Box::new(ToolCallStarted {
            turn: turn(),
            tool_call: id::<ToolCallId>(40),
            acp_id: "t1".into(),
            title: "Read".into(),
            kind: ToolKind::Read,
            tool_name: None,
            status: ToolCallStatus::Pending,
            locations: Vec::new(),
            raw_input: None,
            raw_output: None,
            content: Vec::new(),
        })),
        DomainEvent::ToolCallUpdated(Box::new(ToolCallUpdated {
            tool_call: id::<ToolCallId>(40),
            patch: ToolCallPatch {
                title: Some("Read lease.docx".into()),
                ..ToolCallPatch::default()
            },
        })),
        text(23, TextKind::Answer, "Second answer."),
        DomainEvent::TurnEnded(TurnEnded {
            turn: turn(),
            stop: StopReason::EndTurn,
            usage: None,
            checkpoint_after: None,
        }),
    ]
}

fn fold(envelopes: &[Envelope]) -> (ThreadMemo, Vec<Write>) {
    let mut memo = ThreadMemo::new(thread());
    let mut writes = Vec::new();
    for envelope in envelopes {
        memo.feed(envelope, &mut writes);
    }
    (memo, writes)
}

fn entries(writes: &[Write], wanted: EntryKind) -> Vec<(Seq, String)> {
    writes
        .iter()
        .filter_map(|write| match write {
            Write::Entry {
                order, kind, text, ..
            } if *kind == wanted => Some((*order, text.clone())),
            Write::Entry { .. }
            | Write::ThreadTitle { .. }
            | Write::MatterTitle { .. }
            | Write::Archived { .. }
            | Write::Undone { .. }
            | Write::Files { .. } => None,
        })
        .collect()
}

#[test]
fn answers_are_the_reducers_agent_text_whole_and_only_at_turn_end() {
    let envelopes = envelopes(turn_events());
    let mut state = StreamState::new(StreamId::Thread(thread()));
    // The reducer's agent texts of kind answer, by message: each upsert, then its appends.
    let mut reducer: Vec<(MessageId, Seq, String)> = Vec::new();
    for envelope in &envelopes {
        for change in clauseo_reducer::apply(&mut state, envelope)
            .unwrap()
            .changes
        {
            match change {
                ViewChange::Upsert(entry) => {
                    if let ViewEntry::AgentText(text) = *entry
                        && text.kind == TextKind::Answer
                    {
                        reducer.retain(|(message, _, _)| *message != text.message);
                        reducer.push((text.message, text.order, text.text.clone()));
                    }
                }
                ViewChange::AppendText {
                    message_id, text, ..
                } => {
                    for (message, _, kept) in &mut reducer {
                        if *message == message_id {
                            kept.push_str(&text);
                        }
                    }
                }
                ViewChange::Remove(_) => {}
            }
        }
    }
    let mut reducer: Vec<(Seq, String)> = reducer
        .into_iter()
        .map(|(_, order, text)| (order, text))
        .collect();
    reducer.sort();
    let (_, before_end) = fold(envelopes.split_last().unwrap().1);
    assert_eq!(entries(&before_end, EntryKind::Answer).len(), 0);
    let (_, writes) = fold(&envelopes);
    let answers = entries(&writes, EntryKind::Answer);
    assert_eq!(
        answers,
        vec![
            (seq(5), "Clause 4 is void under s. 23.".to_owned()),
            (seq(10), "Second answer.".to_owned())
        ]
    );
    assert_eq!(answers, reducer);
}

#[test]
fn prompts_tool_titles_and_the_thread_title_are_written_as_they_come() {
    let (_, writes) = fold(&envelopes(turn_events()));
    assert_eq!(
        entries(&writes, EntryKind::Prompt),
        vec![(seq(2), "is clause 4 valid?".to_owned())]
    );
    assert_eq!(
        entries(&writes, EntryKind::ToolTitle),
        vec![
            (seq(8), "Read".to_owned()),
            (seq(8), "Read lease.docx".to_owned())
        ]
    );
    assert!(writes.iter().any(|write| matches!(
        write,
        Write::ThreadTitle { title, .. } if title == "Lease"
    )));
}

#[test]
fn an_edited_queued_prompt_replaces_it_and_an_undone_step_takes_its_entries() {
    let mut events = turn_events();
    events.insert(
        2,
        DomainEvent::QueuedPromptEdited(QueuedPromptEdited {
            turn: turn(),
            message: id::<MessageId>(20),
            content: prompt("is clause 5 valid?"),
        }),
    );
    events.push(DomainEvent::StepUndone(StepUndone {
        turn: turn(),
        checkpoint: id::<CheckpointId>(31),
        restored: Vec::new(),
        kept: Vec::new(),
    }));
    let (_, writes) = fold(&envelopes(events));
    assert_eq!(
        entries(&writes, EntryKind::Prompt),
        vec![
            (seq(2), "is clause 4 valid?".to_owned()),
            (seq(2), "is clause 5 valid?".to_owned())
        ]
    );
    // Shifted by the edit: the prompt at 2, answers at 6 and 11, the tool call at 9.
    assert_eq!(
        writes.last(),
        Some(&Write::Undone {
            thread: thread(),
            entries: vec![seq(2), seq(6), seq(9), seq(11)],
        })
    );
}

#[test]
fn a_newer_generation_supersedes_and_an_older_one_is_refused() {
    let mut generations = Generations::default();
    assert!(generations.submit("main", 2));
    assert!(generations.start("main", 2));
    assert!(generations.submit("main", 3));
    generations.finish();
    assert!(generations.superseded("main", 2));
    assert!(!generations.submit("main", 1));
    assert!(!generations.start("main", 2));
    // Another window is not affected.
    assert!(generations.submit("other", 1));
    assert!(generations.start("other", 1));
}

/// REVIEW REPRO: a steer requeued from turn A to turn B is B's prompt in the view (the reducer
/// sets its `turn` to B); undoing A must not delete it, undoing B must.
#[test]
fn review_a_requeued_steer_belongs_to_its_new_turn() {
    use clauseo_protocol::{SteerOutcome, SteerResolved};
    let b: TurnId = id(3);
    let mut events = turn_events();
    // After the prompt (seq 2) and the start (seq 3): a steer while A runs, requeued to B.
    events.insert(
        3,
        DomainEvent::UserMessage(UserMessage {
            turn: turn(),
            message: id::<MessageId>(50),
            acp_message_id: None,
            content: prompt("steered question"),
            delivery: Delivery::Steer,
        }),
    );
    events.insert(
        4,
        DomainEvent::SteerResolved(SteerResolved {
            message: id::<MessageId>(50),
            outcome: SteerOutcome::Requeued { turn: b },
            aside: Vec::new(),
        }),
    );
    events.push(DomainEvent::StepUndone(StepUndone {
        turn: turn(),
        checkpoint: id::<CheckpointId>(31),
        restored: Vec::new(),
        kept: Vec::new(),
    }));
    let envelopes = envelopes(events);
    // The reducer: the steer (seq 4) is turn B's prompt.
    let mut state = StreamState::new(StreamId::Thread(thread()));
    let mut steer_turn = None;
    for envelope in &envelopes {
        for change in clauseo_reducer::apply(&mut state, envelope)
            .unwrap()
            .changes
        {
            if let ViewChange::Upsert(entry) = change
                && let ViewEntry::UserMessage(user) = *entry
                && user.message == id::<MessageId>(50)
            {
                steer_turn = Some(user.turn);
            }
        }
    }
    assert_eq!(steer_turn, Some(b));
    let (_, writes) = fold(&envelopes);
    let Some(Write::Undone { entries, .. }) = writes.last() else {
        panic!("no undo: {writes:?}");
    };
    assert!(
        !entries.contains(&seq(4)),
        "undoing turn A deletes turn B's prompt (seq 4): {entries:?}"
    );
}

#[test]
fn a_queued_prompt_taken_back_leaves_and_a_cancelled_running_turn_keeps_its_prompt() {
    let queued: TurnId = id(3);
    let user = |turn: TurnId, message: u64, text: &str| {
        DomainEvent::UserMessage(UserMessage {
            turn,
            message: id::<MessageId>(message),
            acp_message_id: None,
            content: prompt(text),
            delivery: Delivery::Queue,
        })
    };
    let cancelled = |turn: TurnId| {
        DomainEvent::TurnEnded(TurnEnded {
            turn,
            stop: StopReason::Cancelled,
            usage: None,
            checkpoint_after: None,
        })
    };
    let mut events = turn_events();
    events.truncate(3); // created, the prompt (seq 2), started
    events.push(user(queued, 60, "queued question")); // seq 4
    events.push(cancelled(queued)); // taken back before it started
    events.push(cancelled(turn())); // the running turn, cancelled
    let (_, writes) = fold(&envelopes(events));
    let undone: Vec<&Write> = writes
        .iter()
        .filter(|write| matches!(write, Write::Undone { .. }))
        .collect();
    assert_eq!(
        undone,
        vec![&Write::Undone {
            thread: thread(),
            entries: vec![seq(4)],
        }]
    );
}
