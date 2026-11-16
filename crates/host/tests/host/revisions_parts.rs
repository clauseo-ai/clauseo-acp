//! Tracked changes outside the main text, and formatting changes, end to end. A Word file whose
//! only changes are in its header, footer and notes or are formatting (`fixtures/revisions/`) is
//! offered Tracked changes once the papers scan has read it; with the fake stetdoc answering what
//! the real 0.9.3 printed for that file, each change is listed with its place, and one is
//! accepted, one rejected and all accepted by Word's change ids.

use clauseo_protocol::{
    CommandOutcome, DecideRevisions, HostCommand, HostEvent, HostQuery, QueryResult,
    RevisionDecision, RevisionKind, RevisionPlace,
};
use futures::StreamExt as _;

use super::{ORIGINAL, Rig, lease};
use crate::settings::{decode, subscribe_app};
use crate::support::{Dir, wait};
use crate::write;

/// Six changes: formatting of the body's first two paragraphs (ids 2, 3), an insertion in the
/// header (11), a deletion in the footer (12), an insertion in a footnote and one in an endnote
/// (13, 14). None is an insertion, deletion or move of the body's text.
const NONBODY_ONLY: &[u8] = include_bytes!("../fixtures/revisions/tracked-nonbody-only.docx");

/// stetdoc 0.9.3's `check --json` of [`NONBODY_ONLY`], trimmed to what the host reads.
const CHECK: &str = r#"{"existing": [{"author": "Reviewer", "changes": [
    {"block": "p1", "date": "2026-11-01T00:00:00Z", "ids": [2], "kind": "run_format"},
    {"block": "p2", "date": "2026-11-01T00:00:00Z", "ids": [3], "kind": "paragraph_format"},
    {"block": "p3", "date": "2026-11-01T00:00:00Z", "ids": [11], "kind": "insertion"},
    {"block": "p4", "date": "2026-11-01T00:00:00Z", "ids": [12], "kind": "deletion"},
    {"block": "p5", "date": "2026-11-01T00:00:00Z", "ids": [13], "kind": "insertion"},
    {"block": "p6", "date": "2026-11-01T00:00:00Z", "ids": [14], "kind": "insertion"}
]}], "exit_status": 0}"#;

/// The ledger lines of its `view --revisions --full --all --json`.
const LEDGER: &[&str] = &[
    "{stet 1 view=ledger mode=redline range=change:2..change:14}",
    "{changes: 6 changes}",
    "#2 format R 2026-11-01 p1",
    "#3 format R 2026-11-01 p2",
    "#11 insert R 2026-11-01 p3 in headers \"inserted\"",
    "#12 delete R 2026-11-01 p4 in headers \"deleted\"",
    "#13 insert R 2026-11-01 p5 in notes \"added\"",
    "#14 insert R 2026-11-01 p6 in notes \"added\"",
];

#[test]
fn changes_outside_the_body_are_listed_with_their_place_and_decided_by_id() {
    let rig = Rig::new("revisions-parts", true);
    write::write(&rig.bin.join("check.json"), CHECK.as_bytes());
    let view = serde_json::json!({"text": LEDGER.join("\n"), "warnings": []});
    write::write(&rig.bin.join("view.json"), view.to_string().as_bytes());
    let list = rig.list("Lease.docx").unwrap();
    let shape: Vec<_> = list
        .revisions
        .iter()
        .map(|r| (r.ids.clone(), r.kind, r.place, r.text.as_deref()))
        .collect();
    let (header, note) = (Some(RevisionPlace::HeaderFooter), Some(RevisionPlace::Note));
    assert_eq!(
        shape,
        [
            (vec![2], RevisionKind::Format, None, None),
            (vec![3], RevisionKind::Format, None, None),
            (vec![11], RevisionKind::Insert, header, Some("inserted")),
            (vec![12], RevisionKind::Delete, header, Some("deleted")),
            (vec![13], RevisionKind::Insert, note, Some("added")),
            (vec![14], RevisionKind::Insert, note, Some("added")),
        ]
    );
    assert_eq!(list.total, 6);

    // Accept the header's insertion: the edit names its id, 11.
    let decided = rig.host.ok(rig.decide(list.version, vec![11]));
    assert!(
        matches!(decided, CommandOutcome::RevisionsDecided { .. }),
        "{decided:?}"
    );
    let mut expected = ORIGINAL.to_vec();
    expected.extend_from_slice(b"@accept\nids: 11\n");
    assert_eq!(rig.lease_bytes(), expected);

    // Reject the first formatting change, from the list read after: its id, 2.
    let after = rig.list("Lease.docx").unwrap();
    let reject = HostCommand::DecideRevisions(DecideRevisions {
        matter: rig.matter,
        path: lease("Lease.docx"),
        version: after.version,
        ids: vec![2],
        decision: RevisionDecision::Reject,
        all: false,
    });
    assert!(matches!(
        rig.host.ok(reject),
        CommandOutcome::RevisionsDecided { .. }
    ));
    expected.extend_from_slice(b"@reject\nids: 2\n");
    assert_eq!(rig.lease_bytes(), expected);

    // Accept all: every id `check` names, the header's, the footer's and the notes' too.
    let after = rig.list("Lease.docx").unwrap();
    assert!(matches!(
        rig.host.ok(rig.decide_all(after.version, Vec::new())),
        CommandOutcome::RevisionsDecided { .. }
    ));
    expected.extend_from_slice(b"@accept\nids: 2, 3, 11, 12, 13, 14\n");
    assert_eq!(rig.lease_bytes(), expected);
    rig.host.shutdown();
}

/// The papers scan reads the real file: no change in the body's text, so before it read the
/// headers, footers, notes and formatting the tile offered no Tracked changes.
#[test]
fn a_file_with_changes_only_outside_the_body_is_offered_tracked_changes() {
    let dir = Dir::new("revisions-parts-tile", &[]);
    let host = dir.start(0);
    let folder = dir.folder("m", &[("plain.txt", "Clause 4\n")]);
    write::write(&folder.join("Notice.docx"), NONBODY_ONLY);
    let (mut app, _) = subscribe_app(&host);
    let matter = host.create_matter("m", &folder);
    let tracked = || match host.query(HostQuery::ListPapers { matter }).unwrap() {
        QueryResult::Papers { papers, .. } => papers
            .iter()
            .find(|paper| paper.path.as_str() == "Notice.docx")
            .is_some_and(|paper| paper.tracked),
        other => panic!("ListPapers answered {other:?}"),
    };
    // The first listing hands the file to the scan, which tells the app when it found marks;
    // the folder's watcher may tell it of other changes first.
    let mut rounds = 0;
    while !tracked() && rounds < 20 {
        rounds += 1;
        loop {
            let item = wait("papers_changed", app.next()).expect("the app stream ended");
            if matches!(decode(&item), HostEvent::PapersChanged { matter: m } if m == matter) {
                break;
            }
        }
    }
    assert!(tracked(), "the tile offers Tracked changes");
    host.shutdown();
}
