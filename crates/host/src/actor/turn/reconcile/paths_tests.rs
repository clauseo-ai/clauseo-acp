//! A repaired turn's places, read back from its log ([`super::turn_paths`]), count as written by
//! the rule the live turn used: a read's location never, an edit's always.

use clauseo_protocol::{
    DomainEvent, Envelope, EventId, MatterPath, Seq, ToolCallId, ToolCallPatch, ToolCallStarted,
    ToolCallStatus, ToolCallUpdated, ToolKind, ToolLocation, TurnId,
};

use crate::pipeline::tests::support::{id, thread_stream, ts};
use crate::sent::Places;

fn inside(path: &str) -> ToolLocation {
    ToolLocation::Inside {
        path: MatterPath::new(path).unwrap(),
        line: None,
    }
}

/// The start of tool call `n` of turn 1, of `kind`, naming `path`.
fn started(n: u64, kind: ToolKind, path: &str) -> DomainEvent {
    DomainEvent::ToolCallStarted(Box::new(ToolCallStarted {
        turn: id::<TurnId>(1),
        tool_call: id::<ToolCallId>(n),
        acp_id: format!("t{n}").into(),
        title: "call".into(),
        kind,
        tool_name: None,
        status: ToolCallStatus::Pending,
        locations: vec![inside(path)],
        raw_input: None,
        raw_output: None,
        content: Vec::new(),
    }))
}

/// An update of tool call `n` naming `path`, its kind left as it was.
fn updated(n: u64, path: &str) -> DomainEvent {
    DomainEvent::ToolCallUpdated(Box::new(ToolCallUpdated {
        tool_call: id::<ToolCallId>(n),
        patch: ToolCallPatch {
            status: Some(ToolCallStatus::Completed),
            locations: Some(vec![inside(path)]),
            ..ToolCallPatch::default()
        },
    }))
}

#[test]
fn a_repaired_turns_reads_are_no_writes() {
    let events = [
        started(1, ToolKind::Read, "note.txt"),
        updated(1, "note.txt"),
        started(2, ToolKind::Edit, "memo.txt"),
        updated(2, "memo2.txt"),
    ];
    let log: Vec<Envelope> = events
        .into_iter()
        .zip(1..)
        .map(|(event, seq)| {
            let event_id = id::<EventId>(1_000 + seq);
            let seq = Seq::new(seq).unwrap();
            Envelope::new(event_id, thread_stream(), seq, ts(0), None, event)
        })
        .collect();
    let written = super::turn_paths(&log, id::<TurnId>(1)).written();
    let expected = Places {
        inside: ["memo.txt", "memo2.txt"]
            .map(|path| MatterPath::new(path).unwrap())
            .into(),
        ..Places::default()
    };
    assert_eq!(written, expected);
}
