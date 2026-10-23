//! The constructors of logged payloads whose fields depend on each other: each refuses the
//! combinations that mean nothing, so the host cannot write one, while decoding still accepts
//! them, so an older or foreign entry reads.

use clauseo_protocol::{
    ChangeKind, CheckpointKind, CheckpointRecorded, ContentId, DomainEvent, FileChange,
    ImportSummary, MatterCreated, MatterKind, TurnQueued,
};

use serde_json::json;

use crate::fixtures::{checkpoint_id, count, path, thread_id, turn_id};

/// The message of a refused value, or a panic naming the value that was built.
fn refusal<T: std::fmt::Debug>(built: Result<T, clauseo_protocol::Error>) -> String {
    match built {
        Ok(value) => panic!("built {value:?}"),
        Err(error) => error.to_string(),
    }
}

#[test]
fn a_checkpoint_names_a_thread_and_turn_only_where_its_kind_has_them() {
    let checkpoint = |kind, thread: Option<u64>, turn: Option<u64>| {
        CheckpointRecorded::new(
            checkpoint_id(8),
            kind,
            thread.map(thread_id),
            turn.map(turn_id),
        )
    };
    let none = "CheckpointRecorded: an import or restore checkpoint names no thread or turn";
    let both = "CheckpointRecorded: a turn's or an undo's checkpoint names its thread and turn";
    let orphan = "CheckpointRecorded: a checkpoint that names a turn names its thread";
    for kind in [CheckpointKind::Import, CheckpointKind::Restore] {
        assert_eq!(refusal(checkpoint(kind, Some(2), None)), none);
        assert_eq!(refusal(checkpoint(kind, None, Some(3))), none);
        assert_eq!(refusal(checkpoint(kind, Some(2), Some(3))), none);
        assert!(checkpoint(kind, None, None).is_ok());
    }
    for kind in [
        CheckpointKind::BeforeTurn,
        CheckpointKind::AfterTurn,
        CheckpointKind::Undo,
    ] {
        assert_eq!(refusal(checkpoint(kind, Some(2), None)), both);
        assert_eq!(refusal(checkpoint(kind, None, Some(3))), both);
        assert_eq!(refusal(checkpoint(kind, None, None)), both);
        assert!(checkpoint(kind, Some(2), Some(3)).is_ok());
    }
    let outside = CheckpointKind::OutsideEdits;
    assert_eq!(refusal(checkpoint(outside, None, Some(3))), orphan);
    assert!(checkpoint(outside, None, None).is_ok());
    assert!(checkpoint(outside, Some(2), None).is_ok());

    let built = checkpoint(outside, Some(2), Some(3)).unwrap();
    assert_eq!(built.checkpoint(), checkpoint_id(8));
    assert_eq!(built.kind(), outside);
    assert_eq!(built.thread(), Some(thread_id(2)));
    assert_eq!(built.turn(), Some(turn_id(3)));
}

#[test]
fn a_file_change_has_the_contents_its_kind_has() {
    let id = || Some(ContentId::new("ab12").unwrap());
    let change =
        |kind, before, after| FileChange::new(path("a.docx"), kind, before, after, count(9));
    let renamed = || ChangeKind::Renamed {
        from: path("old.docx"),
    };
    let added = "FileChange: an added file has content after and none before";
    let deleted = "FileChange: a deleted file has content before and none after";
    let both = "FileChange: a modified or renamed file has content before and after";
    assert_eq!(refusal(change(ChangeKind::Added, id(), id())), added);
    assert_eq!(refusal(change(ChangeKind::Added, None, None)), added);
    assert_eq!(refusal(change(ChangeKind::Deleted, id(), id())), deleted);
    assert_eq!(refusal(change(ChangeKind::Deleted, None, None)), deleted);
    for kind in [ChangeKind::Modified, renamed()] {
        assert_eq!(refusal(change(kind.clone(), None, id())), both);
        assert_eq!(refusal(change(kind.clone(), id(), None)), both);
        assert!(change(kind, id(), id()).is_ok());
    }
    assert!(change(ChangeKind::Deleted, id(), None).is_ok());

    let built = change(ChangeKind::Added, None, id()).unwrap();
    assert_eq!(built.path(), &path("a.docx"));
    assert_eq!(built.change(), &ChangeKind::Added);
    assert_eq!(built.before(), None);
    assert_eq!(built.after(), id().as_ref());
    assert_eq!(built.size_bytes(), count(9));
    assert!(!built.may_include_lawyer_edits());
    assert!(built.with_lawyer_edits().may_include_lawyer_edits());
    let modified = change(ChangeKind::Modified, id(), id()).unwrap();
    assert_eq!(modified.before(), id().as_ref());
}

#[test]
fn only_a_folder_matter_has_an_import_summary() {
    let import = Some(ImportSummary::default());
    assert_eq!(
        refusal(MatterCreated::new("Scratch", MatterKind::Scratch, import)),
        "MatterCreated: the scratch matter has no import summary"
    );
    assert!(MatterCreated::new("Scratch", MatterKind::Scratch, None).is_ok());
    assert!(MatterCreated::new("Smith", MatterKind::Folder, None).is_ok());

    let built = MatterCreated::new("Smith", MatterKind::Folder, import).unwrap();
    assert_eq!(built.name(), "Smith");
    assert_eq!(built.kind(), MatterKind::Folder);
    assert_eq!(built.import(), import);
}

#[test]
fn a_turn_queued_behind_a_turn_waits_in_its_own_thread() {
    let (own, other) = (thread_id(2), thread_id(4));
    let (turn, earlier) = (turn_id(3), turn_id(6));
    assert_eq!(
        refusal(TurnQueued::new(own, turn, other, Some(earlier))),
        "TurnQueued: a turn queued behind a turn waits in its own thread"
    );
    assert_eq!(
        refusal(TurnQueued::new(own, turn, own, Some(turn))),
        "TurnQueued: a turn never waits for itself"
    );
    assert!(TurnQueued::new(own, turn, other, None).is_ok());
    assert!(TurnQueued::new(own, turn, own, None).is_ok());

    let built = TurnQueued::new(own, turn, own, Some(earlier)).unwrap();
    assert_eq!(built.turn(), turn);
    assert_eq!(built.behind(), own);
    assert_eq!(built.behind_turn(), Some(earlier));
}

#[test]
fn decoding_accepts_what_the_constructors_refuse() {
    let (thread, turn, checkpoint) = (thread_id(2), turn_id(3), checkpoint_id(8));
    let file = json!({
        "path": "a.docx",
        "change": "added",
        "before": "ab12",
        "size_bytes": 1,
        "may_include_lawyer_edits": false,
    });
    for value in [
        json!({"checkpoint_recorded": {
            "checkpoint": checkpoint,
            "kind": "import",
            "thread": thread,
        }}),
        json!({"checkpoint_recorded": {"checkpoint": checkpoint, "kind": "undo"}}),
        json!({"matter_created": {
            "name": "S",
            "kind": "scratch",
            "import": {"files": 1, "bytes": 2, "cloud_only": 0},
        }}),
        json!({"turn_queued": {"turn": turn, "behind": thread, "behind_turn": turn}}),
        json!({"outside_edits": {"checkpoint": checkpoint, "files": [file]}}),
    ] {
        let event: DomainEvent = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(serde_json::to_value(&event).unwrap(), value);
    }
}
