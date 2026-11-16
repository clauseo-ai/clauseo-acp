//! The deleted-file limit's history half: an earlier version reads only while the matter held
//! the file ever since, a gap closing only by the lawyer's undo or restore.

use clauseo_protocol::ThreadId;

use super::{Fixture, checkpoint, ok, read, refused, v7};
use crate::versions::LOOKUPS_PER_JOB;
use crate::versions::texts::Refusal;

#[test]
fn a_same_name_file_the_agent_made_after_a_deletion_does_not_open_old_versions() {
    let mut fixture = Fixture::new("versions-cont-agent");
    fixture.put("settlement.txt", b"the figure the lawyer deleted");
    let v2 = fixture.checkpoint(2);
    fixture.remove("settlement.txt");
    fixture.checkpoint(3);
    // The agent writes an empty file of that name in a later turn.
    fixture.put("settlement.txt", b"");
    fixture.after_turn(4, fixture.session.thread);
    assert_eq!(
        refused(&read(&fixture.backend(), v2, "settlement.txt")),
        Refusal::Deleted.text()
    );
    // Not yet checkpointed: the folder's file alone does not open them either.
    let mut fresh = Fixture::new("versions-cont-agent-live");
    fresh.put("settlement.txt", b"the figure the lawyer deleted");
    let v2 = fresh.checkpoint(2);
    fresh.remove("settlement.txt");
    fresh.checkpoint(3);
    fresh.put("settlement.txt", b"");
    assert_eq!(
        refused(&read(&fresh.backend(), v2, "settlement.txt")),
        Refusal::Deleted.text()
    );
    // Both positions of the limit: with deleted files readable, it reads.
    assert_eq!(
        ok(&read(&fresh.backend_deleted(true), v2, "settlement.txt"))["text"],
        "the figure the lawyer deleted"
    );
}

#[test]
fn a_new_file_the_lawyer_made_under_an_old_name_does_not_open_old_versions() {
    let mut fixture = Fixture::new("versions-cont-lawyer");
    fixture.put("notes.txt", b"old notes, deleted");
    let v2 = fixture.checkpoint(2);
    fixture.remove("notes.txt");
    fixture.checkpoint(3);
    fixture.put("notes.txt", b"new notes on another matter of fact");
    fixture.checkpoint(4);
    let backend = fixture.backend();
    assert_eq!(
        refused(&read(&backend, v2, "notes.txt")),
        Refusal::Deleted.text()
    );
    // The new file's own versions read.
    assert_eq!(
        ok(&read(&backend, checkpoint(4), "notes.txt"))["text"],
        "new notes on another matter of fact"
    );
}

#[test]
fn a_restore_from_versions_makes_old_versions_readable_again() {
    let mut fixture = Fixture::new("versions-cont-restore");
    fixture.put("brief.txt", b"the brief as it was");
    let v2 = fixture.checkpoint(2);
    fixture.remove("brief.txt");
    fixture.checkpoint(3);
    let backend = fixture.backend();
    assert_eq!(
        refused(&read(&backend, v2, "brief.txt")),
        Refusal::Deleted.text()
    );
    fixture.restore(v2, &["brief.txt"], 4, 5);
    assert_eq!(
        ok(&read(&backend, v2, "brief.txt"))["text"],
        "the brief as it was"
    );
}

#[test]
fn an_undo_that_brings_back_a_file_the_agent_deleted_makes_old_versions_readable() {
    let mut fixture = Fixture::new("versions-cont-undo");
    fixture.put("draft.txt", b"the draft before the turn");
    let v2 = fixture.checkpoint(2);
    fixture.remove("draft.txt");
    let other = ThreadId::new(v7(77)).unwrap();
    fixture.after_turn(3, other);
    let backend = fixture.backend();
    assert_eq!(
        refused(&read(&backend, v2, "draft.txt")),
        Refusal::Deleted.text()
    );
    fixture.undo((2, 3), &["draft.txt"], 4, 5);
    assert_eq!(
        ok(&read(&backend, v2, "draft.txt"))["text"],
        "the draft before the turn"
    );
}

#[test]
fn the_walk_spans_jobs_and_finds_a_gap_past_the_first() {
    let mut fixture = Fixture::new("versions-cont-long");
    fixture.put("kept.txt", b"kept all along");
    fixture.put("gone.txt", b"gone late");
    let v2 = fixture.checkpoint(2);
    let many = u32::try_from(LOOKUPS_PER_JOB * 3).unwrap();
    for n in 3..3 + many {
        fixture.put("other.txt", "o".repeat(n as usize).as_bytes());
        fixture.checkpoint(n);
    }
    fixture.remove("gone.txt");
    fixture.checkpoint(3 + many);
    fixture.put("gone.txt", b"made again");
    fixture.checkpoint(4 + many);
    let backend = fixture.backend();
    assert_eq!(
        ok(&read(&backend, v2, "kept.txt"))["text"],
        "kept all along"
    );
    assert_eq!(
        refused(&read(&backend, v2, "gone.txt")),
        Refusal::Deleted.text()
    );
}
