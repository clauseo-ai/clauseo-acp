//! The note read from the logs: which turns carried it, and which restores it tells.

use clauseo_protocol::{
    AgentTextEntry, CheckpointId, CompactString, Deliverable, Delivery, DomainEvent, Envelope,
    EventId, MarkerEntry, MarkerKind, MatterPath, MessageId, PermissionEntry, PermissionKind,
    PermissionOption, PermissionRequestId, PermissionState, Seq, StepEntry, StopReason, StreamId,
    TextKind, Timestamp, TurnId, UserMessageEntry, UserMessageStatus, VersionRestored, ViewEntry,
};

use super::{Floor, carried, kept, read, restores_in};
use crate::undo_note::NoteInput;

fn v7(n: u8) -> uuid::Uuid {
    uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &[n; 10]).into_uuid()
}

fn turn(n: u8) -> TurnId {
    TurnId::new(v7(n)).unwrap()
}

fn checkpoint(n: u8) -> CheckpointId {
    CheckpointId::new(v7(100 + n)).unwrap()
}

fn seq(n: u64) -> Seq {
    Seq::new(n).unwrap()
}

fn at(n: i64) -> Timestamp {
    Timestamp::new(1_790_000_000_000 + n).unwrap()
}

/// The lawyer's prompt that opens `turn`.
fn asked(n: u64, turn: TurnId) -> ViewEntry {
    ViewEntry::UserMessage(UserMessageEntry {
        order: seq(n),
        message: MessageId::new(v7(50 + u8::try_from(n).unwrap())).unwrap(),
        turn,
        content: Vec::new(),
        delivery: Delivery::Queue,
        status: UserMessageStatus::Sent,
        edited: false,
        steer: None,
        at_head: false,
        ts: at(0),
        unsent: None,
        copy_of: None,
    })
}

/// The agent's answer text in `turn`.
fn said(n: u64, turn: TurnId) -> ViewEntry {
    ViewEntry::AgentText(AgentTextEntry {
        order: seq(n),
        message: MessageId::new(v7(150 + u8::try_from(n).unwrap())).unwrap(),
        turn,
        kind: TextKind::Answer,
        text: "Done.".to_owned(),
    })
}

/// A permission request of the agent, which names no turn.
fn permission(n: u64) -> ViewEntry {
    ViewEntry::Permission(PermissionEntry {
        order: seq(n),
        request: PermissionRequestId::new(v7(200)).unwrap(),
        tool_call: None,
        options: vec![PermissionOption {
            option_id: CompactString::from("allow"),
            kind: PermissionKind::AllowOnce,
            agent_label: "Yes".to_owned(),
        }],
        deliverable: Deliverable::Live,
        state: PermissionState::Pending,
        about: None,
        helper: None,
        answered_at: None,
    })
}

/// `turn`'s step, stopped so, with `after` its after checkpoint.
fn step(n: u64, turn: TurnId, stop: StopReason, after: Option<CheckpointId>) -> ViewEntry {
    ViewEntry::Step(StepEntry {
        order: seq(n),
        turn,
        stop,
        failure: None,
        files: Vec::new(),
        skipped: Vec::new(),
        checkpoint_before: Some(checkpoint(40 + u8::try_from(n).unwrap())),
        checkpoint_after: after,
        undone: false,
        usage: None,
        ts: at(i64::try_from(n).unwrap()),
        code_mode: false,
        model: None,
        effort: None,
        agent: None,
    })
}

/// An undo of `path`.
fn undone(n: u64, path: &str) -> ViewEntry {
    ViewEntry::Marker(MarkerEntry {
        order: seq(n),
        event: EventId::new(v7(250)).unwrap(),
        kind: MarkerKind::StepUndone {
            turn: turn(9),
            checkpoint: checkpoint(9),
            restored: vec![MatterPath::new(path).unwrap()],
            kept: Vec::new(),
            unversioned: Vec::new(),
        },
        ts: at(0),
    })
}

/// The files of each undo `read` tells, with "|" for a turn that carried the note.
fn told(entries: &[ViewEntry]) -> (Vec<String>, Option<CheckpointId>) {
    let read = read(entries.iter());
    let inputs = read
        .inputs
        .iter()
        .map(|input| match input {
            NoteInput::TurnStarted => "|".to_owned(),
            NoteInput::Undone { restored, .. } => restored[0].as_str().to_owned(),
        })
        .collect();
    (inputs, read.since)
}

#[test]
fn a_turn_counts_when_the_agent_answered_spoke_or_asked() {
    let (one, two, three) = (turn(1), turn(2), turn(3));
    let entries = [
        undone(1, "a.txt"),
        asked(2, one),
        step(3, one, StopReason::EndTurn, Some(checkpoint(1))),
        undone(4, "b.txt"),
        asked(5, two),
        said(6, two),
        step(7, two, StopReason::Cancelled, Some(checkpoint(2))),
        undone(8, "c.txt"),
        asked(9, three),
        permission(10),
        step(11, three, StopReason::Failed, None),
        undone(12, "d.txt"),
    ];
    let (inputs, since) = told(&entries);
    assert_eq!(inputs, ["a.txt", "|", "b.txt", "|", "c.txt", "|", "d.txt"]);
    // The last turn that carried it failed: no after checkpoint, so restores are read from its
    // before checkpoint on.
    assert_eq!(since, None);
    assert_eq!(read(entries.iter()).before, Some(checkpoint(51)));
    assert!(carried(entries.iter(), one));
    assert!(carried(entries.iter(), two));
    assert!(carried(entries.iter(), three));
}

#[test]
fn a_turn_the_agent_never_took_leaves_the_note() {
    let (one, refused, cancelled, own) = (turn(1), turn(2), turn(3), turn(4));
    let entries = [
        asked(1, one),
        said(2, one),
        step(3, one, StopReason::EndTurn, Some(checkpoint(1))),
        undone(4, "a.txt"),
        asked(5, refused),
        step(6, refused, StopReason::Failed, None),
        asked(7, cancelled),
        step(8, cancelled, StopReason::Cancelled, Some(checkpoint(3))),
        // An agent-started turn: it answered, but no prompt carried the note.
        said(9, own),
        step(10, own, StopReason::EndTurn, Some(checkpoint(4))),
    ];
    let (inputs, since) = told(&entries);
    assert_eq!(inputs, ["|", "a.txt"]);
    assert_eq!(since, Some(checkpoint(1)));
    assert_eq!(read(entries.iter()).before, Some(checkpoint(43)));
    // No turn carried it yet: restores since the conversation was created are read.
    let none = read(entries[3..].iter());
    assert_eq!((none.since, none.before), (None, None));
    assert!(!carried(entries.iter(), refused));
    assert!(!carried(entries.iter(), cancelled));
    // A turn whose step is not there yet did not.
    assert!(!carried(entries.iter(), turn(7)));
}

/// `carried` reads only the turn's own entries: what the turn before it said does not count.
#[test]
fn carried_looks_only_at_the_turn_s_own_entries() {
    let (one, two) = (turn(1), turn(2));
    let entries = [
        asked(1, one),
        permission(2),
        said(3, one),
        step(4, one, StopReason::Cancelled, None),
        asked(5, two),
        step(6, two, StopReason::Cancelled, None),
    ];
    assert!(carried(entries.iter(), one));
    assert!(!carried(entries.iter(), two));
    let (inputs, _) = told(&entries);
    assert_eq!(inputs, ["|"]);
}

fn restored(n: u64, checkpoint: CheckpointId, written: &[&str]) -> Envelope {
    let event = VersionRestored {
        from: self::checkpoint(0),
        checkpoint,
        scope: clauseo_protocol::RestoreScope::Matter,
        undoes: None,
        written: written
            .iter()
            .map(|p| MatterPath::new(p).unwrap())
            .collect(),
        removed: Vec::new(),
        deleted: Vec::new(),
        more: None,
    };
    Envelope::new(
        EventId::new(v7(u8::try_from(n).unwrap())).unwrap(),
        StreamId::Matter(clauseo_protocol::MatterId::new(v7(1)).unwrap()),
        seq(n),
        at(i64::try_from(n).unwrap()),
        None,
        DomainEvent::from(event),
    )
}

/// After the turn's checkpoint, a restore whose checkpoint the turn reused came before it; from a
/// time, those logged earlier are left out. A restore that wrote nothing named is not told.
#[test]
fn restores_from_the_floor_on_are_told() {
    let since = checkpoint(1);
    let log = [
        restored(1, since, &["a.txt"]),
        restored(2, checkpoint(2), &[]),
        restored(3, checkpoint(3), &["b.txt"]),
    ];
    let after: Vec<_> = log
        .iter()
        .filter(|envelope| kept(envelope, Floor::After(since)))
        .cloned()
        .collect();
    let restores = restores_in(&after);
    assert_eq!(restores.len(), 1);
    assert_eq!(restores[0].checkpoint, Some(checkpoint(3)));
    assert_eq!(restores[0].back, [MatterPath::new("b.txt").unwrap()]);
    assert_eq!(restores[0].at, at(3));
    let from_time: Vec<_> = log
        .iter()
        .filter(|envelope| kept(envelope, Floor::At(at(2))))
        .map(|envelope| envelope.seq.get())
        .collect();
    assert_eq!(from_time, [2, 3]);
}
