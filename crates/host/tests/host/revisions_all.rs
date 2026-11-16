//! Accept all and Reject all (`DecideRevisions` with `all`), with the fake stetdoc: every change
//! of the version the list was read from, past the list's 500, decided in one write with one
//! `revisions` checkpoint, in chunks of edits when there are more than one edit takes.

use clauseo_protocol::{
    CheckpointKind, CommandOutcome, DomainEvent, ErrorCode, HostCommand, MAX_REVISIONS,
    RestoreRequest,
};

use super::{ORIGINAL, Rig, lease, version_of};
use crate::versions::matter_events;
use crate::write;

/// 600 changes, past the list's 500: Accept all decides every one (in two edits of at most 500
/// ids) with one write and one `revisions` checkpoint, the list read after has none left, and one
/// restore of `before` undoes them all.
#[test]
fn accept_all_decides_all_600_changes_and_one_undo_takes_them_back() {
    let rig = Rig::new("revisions-accept-all", true);
    let count = u32::try_from(MAX_REVISIONS).unwrap() + 100;
    let changes: Vec<_> = (1..=count)
        .map(|id| serde_json::json!({"block": format!("p{id}"), "ids": [id], "kind": "insertion"}))
        .collect();
    let check = serde_json::json!({"existing": [{"author": "Asha Rao", "changes": changes}]});
    write::write(&rig.bin.join("check.json"), check.to_string().as_bytes());
    write::write(&rig.bin.join("check-decided.json"), br#"{"existing": []}"#);
    let list = rig.list("Lease.docx").unwrap();
    assert!(list.truncated);
    assert_eq!(list.revisions.len(), MAX_REVISIONS);
    assert_eq!(list.total, count);

    let outcome = rig.host.ok(rig.decide_all(list.version, Vec::new()));
    let CommandOutcome::RevisionsDecided { before, checkpoint } = outcome else {
        panic!("not decided: {outcome:?}");
    };
    assert!(checkpoint.is_some());
    let mut accepted = ORIGINAL.to_vec();
    for ids in [1..=500, 501..=count] {
        let ids: Vec<String> = ids.map(|id| id.to_string()).collect();
        accepted.extend_from_slice(format!("@accept\nids: {}\n", ids.join(", ")).as_bytes());
    }
    assert_eq!(
        rig.lease_bytes(),
        accepted,
        "the two edits name all 600 ids"
    );
    assert_eq!(rig.calls().iter().filter(|c| *c == "edit").count(), 2);
    rig.host.settle();
    assert_eq!(
        matter_events(&rig.host, rig.matter).checkpoint(CheckpointKind::Revisions),
        checkpoint
    );
    let after = rig.list("Lease.docx").unwrap();
    assert_eq!((after.revisions.len(), after.total), (0, 0));
    assert!(!after.truncated);

    let undo = HostCommand::RestoreVersion {
        thread: None,
        undoes: None,
        matter: rig.matter,
        checkpoint: before,
        scope: RestoreRequest::Files {
            paths: vec![lease("Lease.docx")],
        },
    };
    assert!(matches!(
        rig.host.ok(undo),
        CommandOutcome::VersionRestored { .. }
    ));
    assert_eq!(rig.lease_bytes(), ORIGINAL);
    assert_eq!(rig.list("Lease.docx").unwrap().total, count);
    rig.host.shutdown();
}

/// 1,200 change ids, past one edit's 500: Accept all runs stetdoc's edit in three chunks, each
/// on the one before's output (a replacement's two ids kept together), then writes the file
/// once, with one `revisions` checkpoint; none are left and one restore undoes all three.
#[test]
fn accept_all_of_1200_edits_in_chunks_and_writes_once() {
    let rig = Rig::new("revisions-accept-all-chunks", true);
    // Changes 1 to 1,200, one id each, but 500 and 501 are one change (a replacement).
    let mut changes: Vec<_> = (1..=1_200u32)
        .filter(|id| ![500, 501].contains(id))
        .map(|id| serde_json::json!({"block": format!("p{id}"), "ids": [id], "kind": "insertion"}))
        .collect();
    changes.push(serde_json::json!({"block": "p500", "ids": [501, 500], "kind": "deletion"}));
    let check = serde_json::json!({"existing": [{"author": "Asha Rao", "changes": changes}]});
    write::write(&rig.bin.join("check.json"), check.to_string().as_bytes());
    write::write(&rig.bin.join("check-decided.json"), br#"{"existing": []}"#);
    let list = rig.list("Lease.docx").unwrap();
    assert_eq!(list.total, 1_199);
    let import = matter_events(&rig.host, rig.matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();

    let outcome = rig.host.ok(rig.decide_all(list.version, Vec::new()));
    let CommandOutcome::RevisionsDecided { before, checkpoint } = outcome else {
        panic!("not decided: {outcome:?}");
    };
    assert_eq!(before, import);
    let edits = std::fs::read_to_string(rig.bin.join("edits.txt")).unwrap();
    assert_eq!(
        edits.lines().collect::<Vec<_>>(),
        [
            "read.docx -> out.docx",
            "in.docx -> out.docx",
            "in.docx -> out.docx"
        ],
        "each chunk edits the one before's output"
    );
    let batch = |ids: std::ops::RangeInclusive<u32>| {
        let ids: Vec<String> = ids.map(|id| id.to_string()).collect();
        format!("@accept\nids: {}\n", ids.join(", "))
    };
    let mut accepted = ORIGINAL.to_vec();
    for ids in [1..=499, 500..=999, 1_000..=1_200] {
        accepted.extend_from_slice(batch(ids).as_bytes());
    }
    assert_eq!(
        rig.lease_bytes(),
        accepted,
        "the three chunks' output, written once"
    );
    rig.assert_no_copies();
    rig.host.settle();
    let events = matter_events(&rig.host, rig.matter);
    assert!(checkpoint.is_some());
    assert_eq!(events.checkpoint(CheckpointKind::Revisions), checkpoint);
    let revisions = events.events().filter(|event| {
        matches!(event, DomainEvent::CheckpointRecorded(recorded)
            if recorded.kind() == CheckpointKind::Revisions)
    });
    assert_eq!(revisions.count(), 1, "one checkpoint for the three chunks");
    assert_eq!(rig.list("Lease.docx").unwrap().total, 0);

    let undo = HostCommand::RestoreVersion {
        thread: None,
        undoes: None,
        matter: rig.matter,
        checkpoint: before,
        scope: RestoreRequest::Files {
            paths: vec![lease("Lease.docx")],
        },
    };
    assert!(matches!(
        rig.host.ok(undo),
        CommandOutcome::VersionRestored { .. }
    ));
    assert_eq!(rig.lease_bytes(), ORIGINAL);
    rig.host.shutdown();
}

/// Accept all from a list of bytes the file no longer holds is stale, checked before stetdoc
/// reads anything: nothing is written and no checkpoint taken. All with ids named is refused.
#[test]
fn accept_all_of_a_changed_file_is_stale_and_writes_nothing() {
    let rig = Rig::new("revisions-accept-all-stale", true);
    rig.host.refused(
        rig.decide_all(version_of(b"what the list read"), Vec::new()),
        ErrorCode::WordRevisionsStale,
    );
    assert!(rig.calls().is_empty(), "stetdoc ran: {:?}", rig.calls());
    rig.host.refused(
        rig.decide_all(version_of(ORIGINAL), vec![1]),
        ErrorCode::CommandInvalid,
    );
    assert!(!rig.edited());
    rig.assert_untouched();
    rig.host.settle();
    assert_eq!(
        matter_events(&rig.host, rig.matter).checkpoint(CheckpointKind::Revisions),
        None
    );
    rig.host.shutdown();
}
