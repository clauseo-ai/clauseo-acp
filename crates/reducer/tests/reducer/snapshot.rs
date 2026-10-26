//! Snapshots and folded states: the states the fixtures fold to, the snapshot shape, bytes that
//! round-trip, indexes rebuilt on decoding, and un-durable text left out.

use std::collections::BTreeSet;

use clauseo_protocol::{
    AgentSessionDetached, CompactionChanged, CompactionStatus, ContentAppended, ContentBlock,
    DetachReason, ElicitationOutcome, ElicitationRequest, ElicitationRequested,
    ElicitationResolved, Envelope, ErrorCode, LimitStopReason, Notice, NoticeLevel, Principal,
    StepUndone, TextKind, TextOrBlob, ToolCallStatus, TurnFailed, TurnStoppedAtLimit, ViewEntry,
};
use clauseo_reducer::{
    Error, REDUCER_SCHEMA_VERSION, Snapshot, SnapshotRef, StreamState, apply, view_snapshot,
};

use crate::fixtures::{
    Log, doc_path, files_changed, matter_versions, permission_requested, permission_resolved,
    prompt_stream, scripted_turn, short, start_turn, text, thread_created, tool_started,
    tool_status, turn_ended,
};
use crate::helpers::{agent_text, delta, fold, round_trip};

/// A thread whose entries are of every kind a thread holds: prompt, texts, agent content (an
/// image with inline data, a resource link), tool call, permission, elicitations (pending and
/// resolved), notice, compaction marker with a summary, step, limit stop and undo marker.
#[cfg(test)]
fn every_entry_kind() -> Vec<Envelope> {
    let mut log = Log::thread(12).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Draft the notice «now».").unwrap();
    let message = log.fresh().unwrap();
    log.push(text(turn, message, TextKind::Answer, "Here é:".to_owned()))
        .unwrap();
    let image = ContentBlock::Image {
        data: TextOrBlob::Inline("iVBORw0KGgo=".to_owned()),
        mime_type: short("image/png"),
        uri: None,
    };
    let link = ContentBlock::ResourceLink {
        uri: "file:///matter/lease.docx".to_owned(),
        name: "lease.docx".to_owned(),
        mime_type: None,
        title: Some("Lease".to_owned()),
        description: None,
        size: None,
    };
    for block in [image, link] {
        let kind = TextKind::Answer;
        let event = ContentAppended {
            turn,
            message,
            acp_message_id: None,
            kind,
            block,
        };
        log.push(event).unwrap();
    }
    let tool_call = log.fresh().unwrap();
    log.push(tool_started(turn, tool_call, 4).unwrap()).unwrap();
    let request = log.fresh().unwrap();
    log.push(permission_requested(request, Some(tool_call)))
        .unwrap();
    log.push(permission_resolved(request)).unwrap();
    let elicitations = [log.fresh().unwrap(), log.fresh().unwrap()];
    for elicitation in elicitations {
        let request = ElicitationRequest::Url {
            message: "Sign in to the court portal.".to_owned(),
            url: "https://example.org/login".to_owned(),
        };
        log.push(ElicitationRequested {
            elicitation,
            request,
        })
        .unwrap();
    }
    log.push(ElicitationResolved {
        elicitation: elicitations[1],
        outcome: ElicitationOutcome::Accepted,
        by: Principal::LocalDesktop,
    })
    .unwrap();
    let level = NoticeLevel::Warning;
    let notice = "Context is nearly full.".to_owned();
    log.push(Notice {
        level,
        text: notice,
    })
    .unwrap();
    let summary = Some(TextOrBlob::Inline("Earlier: the lease review.".to_owned()));
    let status = CompactionStatus::Completed;
    log.push(CompactionChanged { status, summary }).unwrap();
    log.push(files_changed(turn).unwrap()).unwrap();
    let checkpoint = log.fresh().unwrap();
    log.push(turn_ended(turn, checkpoint).unwrap()).unwrap();
    let reason = LimitStopReason::Temporary;
    let (window, resets_at) = (None, None);
    log.push(TurnStoppedAtLimit {
        turn,
        window,
        resets_at,
        reason,
    })
    .unwrap();
    let undo = StepUndone {
        turn,
        checkpoint: log.fresh().unwrap(),
        restored: vec![doc_path(4).unwrap()],
        kept: vec![],
    };
    log.push(undo).unwrap();
    log.into_events()
}

/// The `EntryId` kinds of the state's entries.
#[cfg(test)]
fn kinds(state: &StreamState) -> BTreeSet<&'static str> {
    state
        .entries()
        .values()
        .map(|entry| entry.id().kind())
        .collect()
}

/// A thread mid-turn that fills every field of the state: a durable message with un-durable
/// text after it, a message only a delta opened, a pending permission, the turn's files and
/// failure, and the status a detach gives once the turn ends.
#[cfg(test)]
fn mid_turn() -> (Log, StreamState) {
    let mut log = Log::thread(8).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Check the break clause.").unwrap();
    let message = log.fresh().unwrap();
    log.push(text(
        turn,
        message,
        TextKind::Thought,
        "Reading é😀".to_owned(),
    ))
    .unwrap();
    let tool_call = log.fresh().unwrap();
    log.push(tool_started(turn, tool_call, 2).unwrap()).unwrap();
    let request = log.fresh().unwrap();
    log.push(permission_requested(request, Some(tool_call)))
        .unwrap();
    log.push(files_changed(turn).unwrap()).unwrap();
    let failed = TurnFailed {
        turn,
        code: ErrorCode::ALL[0],
        detail: None,
    };
    log.push(failed).unwrap();
    let reason = DetachReason::Closed;
    log.push(AgentSessionDetached { reason }).unwrap();
    let mut state = fold(log.events());
    delta(&mut state, turn, message, TextKind::Thought, " the clause");
    let draft = log.fresh().unwrap();
    delta(&mut state, turn, draft, TextKind::Answer, "Draft");
    (log, state)
}

#[test]
fn prompt_stream_state() {
    insta::assert_json_snapshot!(fold(&prompt_stream().unwrap()));
}

#[test]
fn scripted_turn_state() {
    insta::assert_json_snapshot!(fold(&scripted_turn().unwrap()));
}

#[test]
fn matter_versions_state() {
    insta::assert_json_snapshot!(fold(&matter_versions().unwrap()));
}

#[test]
fn snapshot_shape_is_pinned() {
    let (_, state) = mid_turn();
    let description = "The snapshot shape changed: bump REDUCER_SCHEMA_VERSION \
                       (crates/reducer/src/snapshot.rs), then accept this snapshot.";
    insta::with_settings!({ description => description }, {
        insta::assert_json_snapshot!(Snapshot::of(&state));
    });
}

#[test]
fn snapshot_bytes_round_trip() {
    let every = fold(&every_entry_kind());
    let want = [
        "agent_content",
        "agent_text",
        "elicitation",
        "limit_stop",
        "marker",
        "notice",
        "permission",
        "step",
        "tool_call",
        "user_message",
    ];
    assert_eq!(kinds(&every), want.into_iter().collect());
    let markers = every.entries().values();
    let markers: Vec<_> = markers
        .filter(|e| matches!(e, ViewEntry::Marker(_)))
        .collect();
    assert_eq!(markers.len(), 2, "compaction and undo: {markers:?}");
    let matter = fold(&matter_versions().unwrap());
    assert!(kinds(&matter).contains("marker"), "{:?}", kinds(&matter));
    let logs = [
        prompt_stream(),
        scripted_turn(),
        matter_versions(),
        Ok(every_entry_kind()),
    ];
    for events in logs {
        let state = fold(&events.unwrap());
        let bytes = Snapshot::of(&state).to_bytes().unwrap();
        let read = Snapshot::from_bytes(&bytes).unwrap().into_state().unwrap();
        assert_eq!(read, state);
        // Equal states write equal bytes.
        assert_eq!(Snapshot::of(&read).to_bytes().unwrap(), bytes);
    }

    for garbage in [&b""[..], b"\xc1", b"not a snapshot", b"\x92\x01\xc0"] {
        let read = Snapshot::from_bytes(garbage);
        assert!(matches!(read, Err(Error::SnapshotDecode(_))), "{read:?}");
    }

    let state = fold(&prompt_stream().unwrap());
    let later = REDUCER_SCHEMA_VERSION + 1;
    let newer = SnapshotRef {
        reducer_schema: later,
        state: &state,
    };
    let read = Snapshot::from_bytes(&newer.to_bytes().unwrap()).unwrap();
    let refused = read.into_state();
    assert!(
        matches!(refused, Err(Error::SchemaMismatch { found, expected })
            if found == later && expected == REDUCER_SCHEMA_VERSION),
        "{refused:?}"
    );
    let owned = Snapshot {
        reducer_schema: later,
        state,
    };
    let refused = owned.into_state();
    assert!(
        matches!(refused, Err(Error::SchemaMismatch { .. })),
        "{refused:?}"
    );
}

#[test]
fn snapshot_from_borrow_matches_owned() {
    for events in [prompt_stream(), scripted_turn(), matter_versions()] {
        let state = fold(&events.unwrap());
        let bytes = Snapshot::of(&state).to_bytes().unwrap();
        let expected = Snapshot {
            reducer_schema: REDUCER_SCHEMA_VERSION,
            state: state.clone(),
        };
        assert_eq!(Snapshot::from_bytes(&bytes).unwrap(), expected);
    }
}

#[test]
fn snapshot_rebuilds_indexes() {
    let mut log = Log::thread(9).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let turn = start_turn(&mut log, "Check the rent review.").unwrap();
    let message = log.fresh().unwrap();
    log.push(text(turn, message, TextKind::Answer, "Rent é".to_owned()))
        .unwrap();
    let tool_call = log.fresh().unwrap();
    log.push(tool_started(turn, tool_call, 3).unwrap()).unwrap();
    let request = log.fresh().unwrap();
    log.push(permission_requested(request, Some(tool_call)))
        .unwrap();
    let mut original = fold(log.events());
    let mut decoded = round_trip(&original);
    assert_eq!(decoded, original);

    let tail = [
        log.push(tool_status(tool_call, ToolCallStatus::Completed))
            .unwrap(),
        log.push(permission_resolved(request)).unwrap(),
        log.push(text(
            turn,
            message,
            TextKind::Answer,
            " 😀 rises.".to_owned(),
        ))
        .unwrap(),
        log.push(permission_resolved(request)).unwrap(),
    ];
    for envelope in &tail {
        let want = apply(&mut original, envelope).unwrap();
        let got = apply(&mut decoded, envelope).unwrap();
        assert_eq!(got, want);
        assert!(!want.changes.is_empty() || !want.anomalies.is_empty());
    }
    assert_eq!(decoded, original);
}

#[test]
fn snapshot_drops_undurable_text() {
    let (log, state) = mid_turn();
    let window = view_snapshot(&state);
    let thread = state.as_thread().unwrap();
    let texts: Vec<String> = thread
        .entries()
        .values()
        .filter_map(|entry| match entry {
            clauseo_protocol::ViewEntry::AgentText(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(texts, ["Reading é😀 the clause", "Draft"]);
    assert_eq!(window.entries.len(), state.entries().len());

    let from_log = fold(log.events());
    let read = round_trip(&state);
    assert_eq!(read, from_log);
    let read_window = view_snapshot(&read);
    assert_eq!(read_window.entries.len(), window.entries.len() - 1);
    let message = log
        .events()
        .iter()
        .find_map(|envelope| match &envelope.event {
            clauseo_protocol::DomainEvent::TextAppended(event) => Some(event.message),
            _ => None,
        })
        .unwrap();
    assert_eq!(
        agent_text(&read_window, message).as_deref(),
        Some("Reading é😀")
    );
    assert_eq!(
        agent_text(&window, message).as_deref(),
        Some("Reading é😀 the clause")
    );
}
