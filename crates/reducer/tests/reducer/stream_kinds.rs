//! A stream's kind: an event of the other kind of stream changes nothing and is reported by its
//! own kind, and a state gives the thread's or the matter's state only for its own kind.

use clauseo_protocol::{
    CheckpointKind, CheckpointRecorded, DomainEvent, MarkerKind, MatterCreated, MatterKind,
    MatterRelocated, OutsideEdits, RestoreScope, VersionRestored, ViewEntry,
};
use clauseo_reducer::{Anomaly, ViewChange, apply};

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
    let events: [(DomainEvent, &str); 5] = [
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
        (
            MatterRelocated {
                folder: "/work/Smith v Jones".to_owned(),
            }
            .into(),
            "matter_relocated",
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
fn matter_relocated_folds_to_a_marker_in_the_matter_view() {
    let mut log = Log::matter(21).unwrap();
    let created = MatterCreated::new("Smith v Jones", MatterKind::Folder, None).unwrap();
    log.push(created).unwrap();
    let mut state = fold(log.events());
    let folder = "/work/moved/Smith v Jones".to_owned();
    let envelope = log
        .push(MatterRelocated {
            folder: folder.clone(),
        })
        .unwrap();
    let out = apply(&mut state, &envelope).unwrap();
    assert!(out.anomalies.is_empty(), "{:?}", out.anomalies);
    let marker = match out.changes.as_slice() {
        [ViewChange::Upsert(entry)] => match entry.as_ref() {
            ViewEntry::Marker(marker) => marker.clone(),
            other => panic!("not a marker: {other:?}"),
        },
        other => panic!("changes: {other:?}"),
    };
    assert_eq!(marker.order, envelope.seq);
    assert_eq!(marker.event, envelope.event_id);
    assert_eq!(marker.kind, MarkerKind::MatterRelocated { folder });
    assert!(
        state
            .entries()
            .values()
            .any(|entry| *entry == ViewEntry::Marker(marker.clone())),
        "the marker is not in the state's entries"
    );
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
    assert!(inner.header().is_some());
}
