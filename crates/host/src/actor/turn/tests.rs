//! The provisional order of a message's text against the pipeline, with a real store.

use clauseo_protocol::{
    AgentCapabilities, AgentId, AgentSessionAttached, CheckpointId, Delivery, DomainEvent,
    MatterId, MessageId, Seq, TextKind, ThreadCreated, TurnId, TurnStarted, UserMessage, ViewEntry,
};
use clauseo_reducer::{Anomaly, StreamState, TextDelta, apply_text_delta};
use futures::executor::LocalPool;

use crate::coalesce::{TextIn, ThreadText};
use crate::pipeline::tests::support::{
    Results, TempDir, id, ids, notice, open_store, thread_stream, ts,
};
use crate::pipeline::{Draft, Pipeline, spawn_feeder};

/// The events a turn opens with.
fn opening(turn: TurnId) -> Vec<DomainEvent> {
    vec![
        DomainEvent::ThreadCreated(ThreadCreated {
            matter: id::<MatterId>(2),
            agent: AgentId::new("fake-agent").unwrap(),
            title: None,
        }),
        DomainEvent::AgentSessionAttached(AgentSessionAttached {
            session: "sess-1".parse().unwrap(),
            agent_version: None,
            capabilities: AgentCapabilities::default(),
        }),
        DomainEvent::UserMessage(UserMessage {
            turn,
            message: id::<MessageId>(4),
            acp_message_id: None,
            content: Vec::new(),
            delivery: Delivery::Queue,
        }),
        DomainEvent::TurnStarted(TurnStarted {
            turn,
            checkpoint_before: id::<CheckpointId>(5),
        }),
    ]
}

/// The order key of the entry of `message`.
fn order_of(state: &StreamState, message: MessageId) -> Option<Seq> {
    state.entries().iter().find_map(|(order, entry)| {
        if let ViewEntry::AgentText(text) = entry
            && text.message == message
        {
            Some(*order)
        } else {
            None
        }
    })
}

#[test]
fn delta_order_matches_pipelined_seq() {
    let dir = TempDir::new("delta_order_matches_pipelined_seq");
    let mut pool = LocalPool::new();
    let store = open_store(&mut pool, &dir);
    let (post, mut results) = Results::new();
    let feeder = spawn_feeder(&pool.spawner(), store.clone(), post).unwrap();
    let mut pipeline = Pipeline::<usize>::new(pool.run_until(store.heads()).unwrap());
    let mut ids = ids();
    let stream = thread_stream();
    let mut state = StreamState::new(stream);
    let turn = id::<TurnId>(3);

    // The opening commit returns and folds.
    let mut draft = Draft::new();
    for event in opening(turn) {
        pipeline
            .push(&mut draft, stream, event, None, ts(0), &mut ids)
            .unwrap();
    }
    let ticket = pipeline.send(draft, 0, &feeder).unwrap();
    let (back, result) = results.next(&mut pool);
    assert_eq!(back, ticket);
    for done in pipeline.on_result(back, result).done {
        for envelope in done.outcome.unwrap() {
            clauseo_reducer::apply(&mut state, &envelope).unwrap();
        }
    }

    // Two commits in flight: the writer is held, so neither returns yet.
    let release = pool.run_until(store.hold_writer()).unwrap();
    for tag in 1..=2 {
        let mut draft = Draft::new();
        pipeline
            .push(&mut draft, stream, notice(tag), None, ts(1), &mut ids)
            .unwrap();
        pipeline.send(draft, tag, &feeder).unwrap();
    }
    pool.run_until_stalled();
    assert!(results.ready().is_none(), "a commit returned while held");

    // A new message's first delta: nothing held to close, so it goes straight to the display
    // batch, which shows it with the pending head as `pending_seq`.
    let message = id::<MessageId>(9);
    let mut text = ThreadText::new();
    assert!(text.is_new_message(message));
    let _pushed = text.push(
        std::time::Duration::ZERO,
        TextIn {
            turn,
            message,
            acp_message_id: None,
            kind: TextKind::Answer,
            text: "The clause ".to_owned(),
        },
    );
    let pending = pipeline.pending_head(stream).unwrap();
    let thread = state.as_thread_mut().unwrap();
    let mut show = |shown: crate::coalesce::DisplayText<'_>| {
        let delta = TextDelta {
            turn: shown.turn,
            message: shown.message,
            kind: shown.kind,
            text: shown.text,
            pending_seq: pending,
        };
        apply_text_delta(thread, &delta).map(drop)
    };
    text.flush_display(&mut show).unwrap();
    let provisional = order_of(&state, message).expect("the delta opened the message");

    // The durable flush sends its `TextAppended` first in the next commit.
    let thread = state.as_thread_mut().unwrap();
    let mut show = |shown: crate::coalesce::DisplayText<'_>| {
        let delta = TextDelta {
            turn: shown.turn,
            message: shown.message,
            kind: shown.kind,
            text: shown.text,
            pending_seq: pending,
        };
        apply_text_delta(thread, &delta).map(drop)
    };
    let appended = text.flush_durable(&mut show).unwrap();
    let mut draft = Draft::new();
    let mut seqs = Vec::new();
    for event in appended {
        let seq = pipeline
            .push(
                &mut draft,
                stream,
                DomainEvent::TextAppended(event),
                None,
                ts(2),
                &mut ids,
            )
            .unwrap();
        seqs.push(seq);
    }
    pipeline.send(draft, 3, &feeder).unwrap();
    assert_eq!(
        seqs,
        [provisional],
        "provisional order == the TextAppended seq"
    );

    // Every commit returns; folded in seq order, R6 never corrects the order.
    drop(release);
    let mut anomalies: Vec<Anomaly> = Vec::new();
    let mut folded = 0;
    while folded < 3 {
        let (ticket, result) = results.next(&mut pool);
        for done in pipeline.on_result(ticket, result).done {
            for envelope in done.outcome.unwrap() {
                let applied = clauseo_reducer::apply(&mut state, &envelope).unwrap();
                anomalies.extend(applied.anomalies);
            }
            folded += 1;
        }
    }
    assert!(
        !anomalies
            .iter()
            .any(|anomaly| matches!(anomaly, Anomaly::OrderCorrected { .. })),
        "{anomalies:?}"
    );
    assert_eq!(order_of(&state, message), Some(provisional));
}
