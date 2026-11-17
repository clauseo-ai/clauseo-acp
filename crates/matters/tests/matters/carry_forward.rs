//! A paper Versions holds that iCloud Drive or `OneDrive` later makes online-only, keeping its size
//! and modification time, is carried forward: the next checkpoint's version still holds it, so
//! undo and restore put it back after the agent changes or deletes it. One whose size or
//! modification time changed (a colleague's edit in the cloud) stays as before: its content is
//! unknown, and undo keeps it.

use std::time::{Duration, SystemTime};

use clauseo_matters::CheckpointOutcome;
use clauseo_protocol::{SkippedFile, SkippedFileReason};

use crate::fixture::{
    Fixture, assert_none, cid, created, deleted, meta, modified, path, read_at, tools, undo,
};

/// `rel` written with a modification time long past, so no cache entry is racily clean.
fn old_file(fx: &Fixture, rel: &str, bytes: &[u8]) {
    fx.write(rel, bytes);
    let past = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    crate::support::set_mtime(fx.file(rel), past);
}

/// A matter holding `Lease.docx` (imported as checkpoint 1), then made online-only.
fn evicted(name: &str) -> (Fixture, clauseo_matters::Matter) {
    let fx = Fixture::new(name);
    old_file(&fx, "Lease.docx", b"the lease");
    old_file(&fx, "notes.txt", b"notes");
    let matter = fx.import();
    fx.probe.mark(fx.file("Lease.docx"));
    (fx, matter)
}

#[test]
fn an_evicted_paper_is_carried_forward_and_undo_puts_back_the_agent_s_edit() {
    let (fx, mut matter) = evicted("carry-edit");
    let opening = matter.before_turn(&meta(2)).unwrap();
    // Evicting changes nothing: the import is reused, nothing is skipped.
    assert_eq!(opening.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_none(&opening.skipped);
    // The agent's read downloads it; then it edits it.
    fx.probe.unmark(fx.file("Lease.docx"));
    fx.write("Lease.docx", b"the lease, changed");
    let after = matter
        .after_turn(&meta(3), &tools(&["Lease.docx"]))
        .unwrap();
    assert_eq!(
        created(after, 3),
        [modified("Lease.docx", b"the lease", b"the lease, changed")]
    );
    let undone = undo(&mut matter, (1, 3), &["Lease.docx"], (4, 5)).unwrap();
    assert_eq!(undone.restored, [path("Lease.docx")]);
    assert_none(&undone.unversioned);
    assert_eq!(fx.read("Lease.docx"), b"the lease");
}

#[test]
fn an_evicted_paper_deleted_comes_back_with_undo() {
    let (fx, mut matter) = evicted("carry-delete");
    let opening = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(opening.outcome, CheckpointOutcome::Reused(cid(1)));
    crate::support::remove_file(fx.file("Lease.docx"));
    let after = matter
        .after_turn(&meta(3), &tools(&["Lease.docx"]))
        .unwrap();
    assert_eq!(created(after, 3), [deleted("Lease.docx", b"the lease")]);
    let undone = undo(&mut matter, (1, 3), &["Lease.docx"], (4, 5)).unwrap();
    assert_eq!(undone.restored, [path("Lease.docx")]);
    assert_eq!(fx.read("Lease.docx"), b"the lease");
}

#[test]
fn carried_forward_after_opening_the_matter_again() {
    let (fx, matter) = evicted("carry-reopen");
    drop(matter);
    let mut matter = fx.open(&fx.folder).unwrap();
    let opening = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(opening.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_eq!(read_at(&matter, 1, "Lease.docx"), b"the lease");
}

#[test]
fn a_paper_changed_in_the_cloud_is_not_carried_forward_and_undo_keeps_it() {
    let (fx, mut matter) = evicted("carry-changed");
    // A colleague's edit arrives as a placeholder with another size and time.
    fx.write("Lease.docx", b"the lease, the colleague's");
    let opening = matter.before_turn(&meta(2)).unwrap();
    created(opening, 2);
    fx.probe.unmark(fx.file("Lease.docx"));
    fx.write("Lease.docx", b"the lease, changed by the agent");
    created(
        matter
            .after_turn(&meta(3), &tools(&["Lease.docx"]))
            .unwrap(),
        3,
    );
    let undone = undo(&mut matter, (2, 3), &["Lease.docx"], (4, 5)).unwrap();
    assert_eq!(
        undone.unversioned,
        [SkippedFile {
            path: path("Lease.docx"),
            reason: SkippedFileReason::CloudOnly,
        }]
    );
    assert_eq!(fx.read("Lease.docx"), b"the lease, changed by the agent");
}
