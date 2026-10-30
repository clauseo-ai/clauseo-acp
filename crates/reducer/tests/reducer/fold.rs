//! The thread fold: seq and stream checks, anomalies, the status rules, steps and limit stops,
//! and entries that keep what their events carry.

use clauseo_protocol::{
    AgentSessionDetached, BlobHash, BlobRef, ContentAppended, ContentBlock, Count, DetachReason,
    DomainEvent, EntryId, Envelope, LimitStopReason, ModeChanged, QueuedPromptEdited, StepUndone,
    TextKind, TextOrBlob, ThreadStatus, Timestamp, ToolCallContent, ToolCallPatch, ToolCallStarted,
    ToolCallStatus, ToolCallUpdated, ToolKind, TurnStoppedAtLimit, ViewEntry,
};
use clauseo_reducer::{Anomaly, Error, StreamState, ViewChange, apply, view_snapshot};

use crate::fixtures::{
    Log, file_changes, files_changed, permission_requested, permission_resolved, prompt_stream,
    short, start_turn, text, thread_created, tool_diff, tool_started, tool_status, turn_ended,
    usage, v7_text,
};
use crate::helpers::{fold, seq};

/// A log with its thread created and the state folded so far.
#[cfg(test)]
fn created(n: u64) -> (Log, StreamState) {
    let mut log = Log::thread(n).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let state = fold(log.events());
    (log, state)
}

/// Pushes `event` and applies it, returning its changes and anomalies.
#[cfg(test)]
fn push(
    log: &mut Log,
    state: &mut StreamState,
    event: impl Into<DomainEvent>,
) -> clauseo_reducer::Applied {
    let envelope = log.push(event).unwrap();
    apply(state, &envelope).unwrap()
}

/// Folds what `log` holds past `state`'s `seq` into it.
#[cfg(test)]
fn catch_up(log: &Log, state: &mut StreamState) {
    let from = usize::try_from(state.seq().map_or(0, clauseo_protocol::Seq::get)).unwrap();
    for envelope in &log.events()[from..] {
        apply(state, envelope).unwrap();
    }
}

#[cfg(test)]
fn status(state: &StreamState) -> ThreadStatus {
    state.as_thread().unwrap().status()
}

#[test]
fn seq_gap_is_rejected() {
    let mut log = Log::thread(5).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Read the lease.").unwrap();
    let events = log.events().to_vec();
    let mut state = StreamState::new(log.stream());
    apply(&mut state, &events[0]).unwrap();
    let before = state.clone();

    let gap = apply(&mut state, &events[2]);
    assert!(
        matches!(
            gap,
            Err(Error::SeqOutOfOrder {
                expected: 2,
                got: 3
            })
        ),
        "{gap:?}"
    );
    assert_eq!(state, before);

    let repeat = apply(&mut state, &events[0]);
    assert!(
        matches!(
            repeat,
            Err(Error::SeqOutOfOrder {
                expected: 2,
                got: 1
            })
        ),
        "{repeat:?}"
    );
    assert_eq!(state, before);

    let mut other = Log::thread(6).unwrap();
    other.push(thread_created().unwrap()).unwrap();
    let message = other.fresh().unwrap();
    other
        .push(text(turn, message, TextKind::Answer, "x".to_owned()))
        .unwrap();
    let wrong = apply(&mut state, &other.events()[1]);
    assert!(
        matches!(wrong, Err(Error::WrongStream { expected, got })
            if expected == log.stream() && got == other.stream()),
        "{wrong:?}"
    );
    assert_eq!(state, before);
    // The state takes the next event as usual.
    apply(&mut state, &events[1]).unwrap();
    assert_eq!(state.seq(), Some(seq(2)));
}

#[test]
fn unknown_reference_is_anomaly() {
    let (mut log, mut state) = created(5);
    start_turn(&mut log, "Read the lease.").unwrap();
    let request = log.fresh().unwrap();
    log.push(permission_requested(request, None)).unwrap();
    log.push(permission_resolved(request)).unwrap();
    catch_up(&log, &mut state);
    let before = view_snapshot(&state);

    let unknown = log.fresh().unwrap();
    let out = push(
        &mut log,
        &mut state,
        tool_status(unknown, ToolCallStatus::Failed),
    );
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    let target = EntryId::ToolCall(unknown);
    let event = "tool_call_updated";
    assert_eq!(out.anomalies, [Anomaly::UnknownReference { event, target }]);

    let out = push(&mut log, &mut state, permission_resolved(request));
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    let target = EntryId::Permission(request);
    let event = "permission_resolved";
    assert_eq!(out.anomalies, [Anomaly::AlreadySettled { event, target }]);
    assert_eq!(view_snapshot(&state), before);
    assert_eq!(state.seq(), Some(seq(7)));
}

#[test]
fn unknown_kind_advances_seq() {
    let (log, mut state) = created(5);
    let before = state.clone();
    let json = format!(
        r#"{{"schema_version":1,"event_id":"{}","stream":{},"seq":2,"ts":2000,"event":{}}}"#,
        v7_text(5 * crate::fixtures::ID_SPACE + 900),
        serde_json::to_string(&log.stream()).unwrap(),
        r#"{"later_kind":{"x":1,"y":[true,null]}}"#,
    );
    let envelope: Envelope = serde_json::from_str(&json).unwrap();
    assert!(matches!(envelope.event, DomainEvent::Unknown { .. }));
    let out = apply(&mut state, &envelope).unwrap();
    assert_eq!(out, clauseo_reducer::Applied::default());
    assert_eq!(state.seq(), Some(seq(2)));
    // Everything but seq is as before.
    let without_seq = |state: &StreamState| {
        let mut value = serde_json::to_value(state).unwrap();
        value["thread"]["seq"] = serde_json::Value::Null;
        value
    };
    assert_eq!(without_seq(&state), without_seq(&before));
    assert_eq!(view_snapshot(&state), view_snapshot(&before));
}

#[test]
fn later_event_is_not_yet_folded() {
    let (mut log, mut state) = created(5);
    let turn = start_turn(&mut log, "Read the lease.").unwrap();
    catch_up(&log, &mut state);
    let before = view_snapshot(&state);
    let message = log.fresh().unwrap();
    let edited = QueuedPromptEdited {
        turn,
        message,
        content: vec![ContentBlock::Text {
            text: "Read clause 2.".to_owned(),
        }],
    };
    let out = push(&mut log, &mut state, edited);
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(
        out.anomalies,
        [Anomaly::NotYetFolded {
            event: "queued_prompt_edited"
        }]
    );
    assert_eq!(state.seq(), Some(seq(4)));
    assert_eq!(view_snapshot(&state), before);
}

#[test]
fn status_follows_the_turn() {
    let (mut log, mut state) = created(5);
    assert_eq!(status(&state), ThreadStatus::Idle);
    let turn = log.fresh().unwrap();
    let message = log.fresh().unwrap();
    push(
        &mut log,
        &mut state,
        crate::fixtures::user_message(turn, message, "Go."),
    );
    assert_eq!(status(&state), ThreadStatus::Starting);
    let checkpoint_before = log.fresh().unwrap();
    let started = clauseo_protocol::TurnStarted {
        turn,
        checkpoint_before,
    };
    push(&mut log, &mut state, started);
    assert_eq!(status(&state), ThreadStatus::Running);
    let request = log.fresh().unwrap();
    let out = push(&mut log, &mut state, permission_requested(request, None));
    assert_eq!(status(&state), ThreadStatus::NeedsYou);
    assert!(out.changes.iter().any(|c| matches!(c,
        ViewChange::Upsert(e) if matches!(**e, ViewEntry::Header(ref h)
            if h.status == ThreadStatus::NeedsYou))));
    push(&mut log, &mut state, permission_resolved(request));
    assert_eq!(status(&state), ThreadStatus::Running);
    let after = log.fresh().unwrap();
    push(&mut log, &mut state, turn_ended(turn, after).unwrap());
    assert_eq!(status(&state), ThreadStatus::Idle);
}

#[test]
fn unknown_detach_reason_gives_agent_exited() {
    let reason: DetachReason = serde_json::from_str(r#""crashed_in_a_later_version""#).unwrap();
    assert!(matches!(reason, DetachReason::Unknown { .. }), "{reason:?}");
    let (mut log, mut state) = created(5);
    push(&mut log, &mut state, AgentSessionDetached { reason });
    assert_eq!(status(&state), ThreadStatus::AgentExited);

    let (mut log, mut state) = created(6);
    let exited = DetachReason::Exited {
        code: Some(1),
        stderr_tail: String::new(),
    };
    push(
        &mut log,
        &mut state,
        AgentSessionDetached { reason: exited },
    );
    assert_eq!(status(&state), ThreadStatus::AgentExited);
    let closed = DetachReason::Closed;
    push(
        &mut log,
        &mut state,
        AgentSessionDetached { reason: closed },
    );
    assert_eq!(status(&state), ThreadStatus::Idle);
}

#[test]
fn agent_content_gets_own_entry() {
    let (mut log, mut state) = created(5);
    let turn = start_turn(&mut log, "Show the plan.").unwrap();
    let message = log.fresh().unwrap();
    log.push(text(
        turn,
        message,
        TextKind::Answer,
        "Here it is:".to_owned(),
    ))
    .unwrap();
    catch_up(&log, &mut state);
    let text_before = crate::helpers::shown_text(&state, message);
    let block = ContentBlock::Text {
        text: "a block".to_owned(),
    };
    let event = ContentAppended {
        turn,
        message,
        acp_message_id: None,
        kind: TextKind::Answer,
        block: block.clone(),
    };
    let envelope = log.push(event).unwrap();
    let out = apply(&mut state, &envelope).unwrap();
    assert_eq!(out.changes.len(), 1);
    let ViewEntry::AgentContent(entry) = crate::helpers::first_upsert(&out.changes) else {
        panic!("not agent content: {:?}", out.changes);
    };
    let id = ViewEntry::AgentContent(entry.clone()).id();
    assert_eq!(
        id.to_string(),
        format!("agent_content:{}", envelope.event_id)
    );
    let event_uuid = uuid::Uuid::parse_str(&envelope.event_id.to_string()).unwrap();
    assert_eq!(id.uuid(), event_uuid);
    assert_eq!(
        (entry.message, entry.block, entry.order),
        (message, block, envelope.seq)
    );
    assert_eq!(crate::helpers::shown_text(&state, message), text_before);
    assert_eq!(state.entries().len(), 3);
}

#[cfg(test)]
fn blob() -> TextOrBlob {
    let hash = BlobHash::from_bytes([7; 32]);
    let blob = BlobRef::new(
        hash,
        Count::new(100_000).unwrap(),
        "First lines…".to_owned(),
    );
    TextOrBlob::Blob(blob.unwrap())
}

#[cfg(test)]
fn tool_entry(state: &StreamState) -> Box<clauseo_protocol::ToolCallEntry> {
    state
        .entries()
        .values()
        .find_map(|e| match e {
            ViewEntry::ToolCall(call) => Some(call.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn tool_call_blob_kept_as_ref() {
    let (mut log, mut state) = created(5);
    let turn = start_turn(&mut log, "Read the bundle.").unwrap();
    let tool_call = log.fresh().unwrap();
    let started = ToolCallStarted {
        turn,
        tool_call,
        acp_id: short("toolu_blob"),
        title: short("Read bundle"),
        kind: ToolKind::Read,
        tool_name: None,
        status: ToolCallStatus::InProgress,
        locations: vec![],
        raw_input: None,
        raw_output: Some(blob()),
        content: vec![ToolCallContent::Text { text: blob() }],
    };
    log.push(Box::new(started)).unwrap();
    catch_up(&log, &mut state);
    let entry = tool_entry(&state);
    assert_eq!(entry.raw_output, Some(blob()));
    assert_eq!(entry.content, [ToolCallContent::Text { text: blob() }]);

    let patch = ToolCallPatch {
        raw_input: Some(blob()),
        ..ToolCallPatch::default()
    };
    let out = push(
        &mut log,
        &mut state,
        Box::new(ToolCallUpdated { tool_call, patch }),
    );
    let ViewEntry::ToolCall(sent) = crate::helpers::first_upsert(&out.changes) else {
        panic!("not a tool call");
    };
    assert_eq!(sent.raw_input, Some(blob()));
    assert_eq!(tool_entry(&state).raw_input, Some(blob()));
}

#[test]
fn no_patch_for_unchanged_entry() {
    let (mut log, mut state) = created(5);
    let turn = start_turn(&mut log, "Go.").unwrap();
    let tool_call = log.fresh().unwrap();
    log.push(tool_started(turn, tool_call, 1).unwrap()).unwrap();
    catch_up(&log, &mut state);
    let mode = || ModeChanged {
        mode_id: short("plan"),
    };
    assert_eq!(push(&mut log, &mut state, mode()).changes.len(), 1);
    let out = push(&mut log, &mut state, mode());
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(
        push(&mut log, &mut state, usage(1000).unwrap())
            .changes
            .len(),
        1
    );
    let out = push(&mut log, &mut state, usage(1000).unwrap());
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    let done = || tool_status(tool_call, ToolCallStatus::Completed);
    assert_eq!(push(&mut log, &mut state, done()).changes.len(), 1);
    let out = push(&mut log, &mut state, done());
    assert!(out.changes.is_empty(), "{:?}", out.changes);
    assert_eq!(
        push(&mut log, &mut state, tool_diff(tool_call, 1).unwrap())
            .changes
            .len(),
        1
    );
    let again = push(&mut log, &mut state, tool_diff(tool_call, 1).unwrap());
    assert!(again.changes.is_empty() && again.anomalies.is_empty());
}

#[cfg(test)]
fn step(state: &StreamState) -> clauseo_protocol::StepEntry {
    state
        .entries()
        .values()
        .rev()
        .find_map(|e| match e {
            ViewEntry::Step(step) => Some(step.clone()),
            _ => None,
        })
        .unwrap()
}

#[test]
fn step_without_files_changed() {
    let mut state = fold(&prompt_stream().unwrap());
    let last = state.entries().values().next_back().unwrap().clone();
    assert!(matches!(last, ViewEntry::Step(ref s) if s.files.is_empty() && s.skipped.is_empty()));
    assert_eq!(status(&state), ThreadStatus::Idle);

    let (mut log, _) = created(5);
    let turn = start_turn(&mut log, "Fix the lease.").unwrap();
    log.push(files_changed(turn).unwrap()).unwrap();
    let checkpoint = log.fresh().unwrap();
    log.push(turn_ended(turn, checkpoint).unwrap()).unwrap();
    state = fold(log.events());
    let ended = step(&state);
    assert_eq!(ended.files, file_changes().unwrap());
    assert!(!ended.undone);
    assert_eq!(status(&state), ThreadStatus::Idle);

    let undo = StepUndone {
        turn,
        checkpoint: log.fresh().unwrap(),
        restored: vec![crate::fixtures::doc_path(1).unwrap()],
        kept: vec![],
    };
    let entries = state.entries().len();
    let out = push(&mut log, &mut state, undo);
    assert!(step(&state).undone);
    assert_eq!(state.entries().len(), entries + 1);
    let marker = state.entries().values().next_back().unwrap();
    assert!(matches!(marker, ViewEntry::Marker(_)), "{marker:?}");
    assert_eq!(out.changes.len(), 2);
}

#[test]
fn paused_at_limit_after_turn_ended() {
    let resets_at = Some(Timestamp::new(1_760_000_000_000).unwrap());
    let stop = |turn| TurnStoppedAtLimit {
        turn,
        window: Some("5h".to_owned()),
        resets_at,
        reason: LimitStopReason::PlanWindow,
    };
    let paused = ThreadStatus::PausedAtLimit { resets_at };
    let has_card = |state: &StreamState| {
        state
            .entries()
            .values()
            .any(|e| matches!(e, ViewEntry::LimitStop(s) if s.resets_at == resets_at))
    };

    // After TurnEnded.
    let (mut log, mut state) = created(5);
    let turn = start_turn(&mut log, "Go.").unwrap();
    let after = log.fresh().unwrap();
    log.push(turn_ended(turn, after).unwrap()).unwrap();
    catch_up(&log, &mut state);
    assert_eq!(status(&state), ThreadStatus::Idle);
    push(&mut log, &mut state, stop(turn));
    assert_eq!(status(&state), paused);
    assert!(has_card(&state));

    // Before TurnEnded: still paused once it ends.
    let (mut log, mut state) = created(6);
    let turn = start_turn(&mut log, "Go.").unwrap();
    catch_up(&log, &mut state);
    push(&mut log, &mut state, stop(turn));
    assert_eq!(status(&state), paused);
    let after = log.fresh().unwrap();
    push(&mut log, &mut state, turn_ended(turn, after).unwrap());
    assert_eq!(status(&state), paused);
    assert!(has_card(&state));
}
