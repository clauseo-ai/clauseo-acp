//! A stream's kind: an event of the other kind of stream changes nothing and is reported by its
//! own kind, and a state gives the thread's or the matter's state only for its own kind.

use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, DomainEvent, MatterCreated, MatterKind, OutsideEdits,
    RestoreScope, VersionRestored,
};
use clauseo_reducer::{Anomaly, apply};

use crate::fixtures::{
    Log, file_changes, matter_versions, prompt_stream, thread_created, user_message,
};
use crate::helpers::fold;

#[test]
fn thread_event_on_a_matter_stream_is_reported_by_its_kind() {
    let mut log = Log::matter(21).unwrap();
    let created = MatterCreated::new("Smith v Jones", MatterKind::Scratch, None).unwrap();
    log.push(created).unwrap();
    let mut state = fold(log.events());
    let (turn, message) = (log.fresh().unwrap(), log.fresh().unwrap());
    let events: [(DomainEvent, &str); 2] = [
        (thread_created().unwrap(), "thread_created"),
        (user_message(turn, message, "Draft."), "user_message"),
    ];
    for (event, kind) in events {
        let before = state.clone();
        let out = apply(&mut state, &log.push(event).unwrap()).unwrap();
        assert!(out.changes.is_empty(), "{:?}", out.changes);
        assert_eq!(
            out.anomalies,
            vec![Anomaly::WrongStreamKind { event: kind }]
        );
        assert_eq!(state.entries(), before.entries());
        assert_eq!(
            state.as_matter().unwrap().header(),
            before.as_matter().unwrap().header()
        );
    }
}

#[test]
fn matter_event_on_a_thread_stream_is_reported_by_its_kind() {
    let mut log = Log::thread(21).unwrap();
    log.push(thread_created().unwrap()).unwrap();
    let mut state = fold(log.events());
    let (checkpoint, from) = (log.fresh().unwrap(), log.fresh().unwrap());
    let events: [(DomainEvent, &str); 4] = [
        (
            MatterCreated::new("Smith v Jones", MatterKind::Scratch, None)
                .unwrap()
                .into(),
            "matter_created",
        ),
        (
            CheckpointRecorded::new(checkpoint, CheckpointKind::Import, None, None)
                .unwrap()
                .into(),
            "checkpoint_recorded",
        ),
        (
            OutsideEdits {
                checkpoint,
                files: file_changes().unwrap(),
            }
            .into(),
            "outside_edits",
        ),
        (
            VersionRestored {
                from,
                checkpoint,
                scope: RestoreScope::Matter,
            }
            .into(),
            "version_restored",
        ),
    ];
    for (event, kind) in events {
        let before = state.clone();
        let out = apply(&mut state, &log.push(event).unwrap()).unwrap();
        assert!(out.changes.is_empty(), "{:?}", out.changes);
        assert_eq!(
            out.anomalies,
            vec![Anomaly::WrongStreamKind { event: kind }]
        );
        assert_eq!(state.entries(), before.entries());
    }
}

#[test]
fn a_state_gives_its_own_kind_with_its_seq_and_header() {
    let matter = fold(&matter_versions().unwrap());
    assert!(matter.as_thread().is_none());
    let inner = matter.as_matter().unwrap();
    assert!(inner.seq().is_some());
    assert_eq!(inner.seq(), matter.seq());
    assert_eq!(inner.header().unwrap().name, "Smith v Jones");
    assert_eq!(inner.entries(), matter.entries());

    let thread = fold(&prompt_stream().unwrap());
    assert!(thread.as_matter().is_none());
    let inner = thread.as_thread().unwrap();
    assert!(inner.seq().is_some());
    assert_eq!(inner.seq(), thread.seq());
}
