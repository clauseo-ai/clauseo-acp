//! `update_during_inflight_commit_is_kept`: the pipeline, the sent view and the builder together.

use clauseo_protocol::{
    AgentCapabilities, AgentId, AgentSessionAttached, AgentSessionId, CheckpointId, Delivery,
    DomainEvent, Envelope, INLINE_LIMIT_BYTES, MatterId, MessageId, StreamId, TextOrBlob,
    ThreadCreated, ToolCallContent, ToolCallId, ToolCallPatch, ToolCallStatus, ToolCallUpdated,
    ToolKind, TurnId, TurnStarted, UserMessage, ViewEntry,
};
use clauseo_reducer::StreamState;
use clauseo_store::BlobIn;
use futures::executor::LocalPool;

use super::support::{Results, TempDir, id, ids, open_store, thread_stream, ts};
use super::{receipt, stored_seqs};
use crate::build::{self, ToolFields};
use crate::ids::Ids;
use crate::pipeline::{Draft, FeederTx, Pipeline, Ticket, spawn_feeder};
use crate::sent::SentView;

/// The actor's side of the test: the pipeline, the reducer state and the sent view of one
/// thread, moved the way the actor moves them.
struct Actor {
    pipeline: Pipeline<usize>,
    feeder: FeederTx,
    results: Results,
    ids: Ids,
    stream: StreamId,
    state: StreamState,
    sent: SentView,
}

impl Actor {
    /// Sends `events` (with `blobs`, and command 1's receipt when `reused`) as one commit and
    /// folds them into the sent view.
    fn send(&mut self, events: Vec<DomainEvent>, blobs: Vec<BlobIn>, reused: bool) -> Ticket {
        let mut draft = Draft::new();
        for event in events {
            self.pipeline
                .push(&mut draft, self.stream, event, None, ts(1), &mut self.ids)
                .unwrap();
        }
        draft.blobs(blobs);
        if reused {
            draft.receipt(receipt(1));
        }
        let ticket = self.pipeline.send(draft, 0, &self.feeder).unwrap();
        self.sent.on_sent(self.pipeline.events(ticket));
        ticket
    }

    /// Waits for `n` results and handles each: apply and fold what returned `Ok`, roll the sent
    /// view back for a stream that drained after a failure. Returns how many commits failed.
    fn settle(&mut self, pool: &mut LocalPool, n: usize) -> usize {
        let mut failed = 0;
        for _ in 0..n {
            let (ticket, result) = self.results.next(pool);
            let completed = self.pipeline.on_result(ticket, result);
            for done in completed.done {
                match done.outcome {
                    Ok(envelopes) => self.committed(&envelopes),
                    Err(_) => failed += 1,
                }
            }
            if completed.unblocked.contains(&self.stream) {
                self.sent.roll_back();
            }
        }
        failed
    }

    fn committed(&mut self, envelopes: &[Envelope]) {
        for envelope in envelopes {
            clauseo_reducer::apply(&mut self.state, envelope).unwrap();
        }
        self.sent.on_committed(envelopes);
    }
}

fn session() -> AgentSessionId {
    AgentSessionId::new("session-1").unwrap()
}

/// A tool call whose input takes `input_bytes` of the inline limit.
fn tool_call(acp_id: &str, input_bytes: usize) -> clauseo_acp::ToolCall {
    clauseo_acp::ToolCall {
        acp_id: acp_id.into(),
        title: "Read brief.txt".into(),
        kind: ToolKind::Read,
        tool_name: None,
        status: ToolCallStatus::InProgress,
        locations: Vec::new(),
        raw_input: Some(TextOrBlob::Inline("i".repeat(input_bytes))),
        raw_output: None,
        content: Vec::new(),
    }
}

/// A patch replacing the content with `bytes` of text.
fn content_patch(bytes: usize) -> ToolCallPatch {
    ToolCallPatch {
        content: Some(vec![ToolCallContent::Text {
            text: TextOrBlob::Inline("c".repeat(bytes)),
        }]),
        ..ToolCallPatch::default()
    }
}

/// The opening events: thread, session, prompt, turn start.
fn opening(turn: TurnId) -> Vec<DomainEvent> {
    vec![
        DomainEvent::ThreadCreated(ThreadCreated {
            matter: id::<MatterId>(2),
            agent: AgentId::new("fake-agent").unwrap(),
            title: None,
        }),
        DomainEvent::AgentSessionAttached(AgentSessionAttached {
            session: session(),
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

#[test]
fn update_during_inflight_commit_is_kept() {
    let dir = TempDir::new("update_during_inflight_commit_is_kept");
    let mut pool = LocalPool::new();
    let store = open_store(&mut pool, &dir);
    let (post, results) = Results::new();
    let stream = thread_stream();
    let mut actor = Actor {
        pipeline: Pipeline::new(pool.run_until(store.heads()).unwrap()),
        feeder: spawn_feeder(&pool.spawner(), store.clone(), post).unwrap(),
        results,
        ids: ids(),
        stream,
        state: StreamState::new(stream),
        sent: SentView::new(stream),
    };
    let turn = id::<TurnId>(3);
    actor.send(opening(turn), Vec::new(), true);
    assert_eq!(actor.settle(&mut pool, 1), 0);

    // ToolCallStarted is sent and held in flight; its input takes 3,000 of the 4,096 bytes.
    let release = pool.run_until(store.hold_writer()).unwrap();
    let tool = id::<ToolCallId>(6);
    let (started, sent_fields) = build::tool_call_started(turn, tool, tool_call("call-1", 3_000));
    assert_eq!(started.blobs.len(), 0);
    actor.send(
        vec![DomainEvent::ToolCallStarted(Box::new(started.value))],
        started.blobs,
        false,
    );

    // The update arrives before that commit returns: the sent view knows the call, and its
    // fields are the ones ToolCallStarted sent.
    let (found, known) = actor
        .sent
        .tool_call(&session(), "call-1")
        .expect("kept, not dropped");
    assert_eq!(found, tool);
    assert_eq!(known.fields, sent_fields);
    assert!(
        !actor
            .state
            .entries()
            .values()
            .any(|entry| matches!(entry, ViewEntry::ToolCall(_))),
        "nothing of it is committed yet"
    );
    let (updated, _) = build::tool_call_updated(tool, content_patch(2_000), known.fields);
    // Counted against the 3,000 bytes already sent, 2,000 more do not fit: a blob. Counted
    // against stale committed fields (none), they would have stayed inline.
    assert_eq!(updated.blobs.len(), 1);
    let (stale, _) = build::tool_call_updated(tool, content_patch(2_000), ToolFields::default());
    assert_eq!(stale.blobs.len(), 0);
    actor.send(
        vec![DomainEvent::ToolCallUpdated(Box::new(updated.value))],
        updated.blobs,
        false,
    );
    release.send(()).unwrap();
    assert_eq!(actor.settle(&mut pool, 2), 0);

    let entry = tool_entry(&actor.state, tool);
    assert!(matches!(
        entry.content.as_slice(),
        [ToolCallContent::Text {
            text: TextOrBlob::Blob(_)
        }]
    ));
    assert!(ViewEntry::ToolCall(Box::new(entry)).inline_len() <= INLINE_LIMIT_BYTES);
    assert!(actor.sent.settled());

    // A failed commit (command 1's id again) and the commit after it: the sent view shows both
    // while they are in flight, and rolls back once the stream drains.
    let release = pool.run_until(store.hold_writer()).unwrap();
    let finish = ToolCallPatch {
        status: Some(ToolCallStatus::Completed),
        ..ToolCallPatch::default()
    };
    let done = DomainEvent::ToolCallUpdated(Box::new(ToolCallUpdated {
        tool_call: tool,
        patch: finish,
    }));
    actor.send(vec![done], Vec::new(), true);
    let (second, _) = build::tool_call_started(turn, id::<ToolCallId>(7), tool_call("call-2", 10));
    actor.send(
        vec![DomainEvent::ToolCallStarted(Box::new(second.value))],
        Vec::new(),
        false,
    );
    assert!(actor.sent.tool(tool).unwrap().finished);
    assert!(actor.sent.tool_call(&session(), "call-2").is_some());
    release.send(()).unwrap();
    assert_eq!(actor.settle(&mut pool, 2), 2);

    assert!(actor.sent.settled());
    assert!(actor.sent.tool_call(&session(), "call-2").is_none());
    let log = pool.run_until(store.read_after(stream, None, 500)).unwrap();
    assert_eq!(stored_seqs(&mut pool, &store, stream), [1, 2, 3, 4, 5, 6]);
    assert!(actor.sent.same_as(&SentView::rebuild(stream, &log)));
    // The sent view matches the reducer state: the same tool calls, costs and statuses.
    let calls: Vec<_> = actor
        .state
        .entries()
        .values()
        .filter_map(|entry| {
            if let ViewEntry::ToolCall(call) = entry {
                Some(call)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(calls.len(), 1);
    for call in calls {
        let sent = actor.sent.tool(call.tool_call).unwrap();
        let cost = ViewEntry::ToolCall(call.clone()).inline_len();
        assert_eq!(sent.fields.inline_len(), cost);
        assert_eq!(sent.finished, call.status == ToolCallStatus::Completed);
        assert!(!sent.finished);
    }

    drop(actor);
    pool.run_until(store.close()).unwrap();
}

/// The reducer's entry for `tool`.
fn tool_entry(state: &StreamState, tool: ToolCallId) -> clauseo_protocol::ToolCallEntry {
    state
        .entries()
        .values()
        .find_map(|entry| {
            if let ViewEntry::ToolCall(call) = entry
                && call.tool_call == tool
            {
                Some((**call).clone())
            } else {
                None
            }
        })
        .unwrap()
}
