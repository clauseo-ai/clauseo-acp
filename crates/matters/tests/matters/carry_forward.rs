//! A paper Versions holds that a sync client later makes online-only, keeping its size and
//! modification time (measured for iCloud Drive; expected, not yet measured, for `OneDrive`), is
//! carried forward: the next checkpoint's version still holds it, so undo and restore put it
//! back after the agent changes or deletes it. One whose size or modification time changed (as
//! a colleague's edit in the cloud usually does) stays as before: its content is unknown, and
//! undo keeps it. A carried placeholder itself is never written while it is online-only.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

use clauseo_matters::{
    CheckpointOutcome, Error, Imported, Matter, PlaceholderProbe, ReplaceFile, Settings,
};
use clauseo_protocol::{ErrorCode, SkippedFile, SkippedFileReason};

use crate::fixture::{
    Fixture, assert_no_temp_files, assert_none, at, cid, created, deleted, meta, modified, path,
    read_at, tools, undo,
};

/// The modification time every test file starts with: long past, so no cache entry is racily
/// clean.
const PAST: Duration = Duration::from_secs(1_700_000_000);

/// `rel` written with a modification time `since` the epoch.
fn file_at(fx: &Fixture, rel: &str, bytes: &[u8], since: Duration) {
    fx.write(rel, bytes);
    crate::support::set_mtime(fx.file(rel), SystemTime::UNIX_EPOCH + since);
}

/// `rel` written with the modification time [`PAST`].
fn old_file(fx: &Fixture, rel: &str, bytes: &[u8]) {
    file_at(fx, rel, bytes, PAST);
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

/// The stat cache read back from disk keeps the stat each file was read with, so a matter
/// opened again carries the placeholder forward too, and undo puts back the agent's delete.
#[test]
fn carried_forward_after_opening_the_matter_again_and_undo_puts_it_back() {
    let (fx, matter) = evicted("carry-reopen");
    drop(matter);
    let mut matter = fx.open(&fx.folder).unwrap();
    let opening = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(opening.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_none(&opening.skipped);
    crate::support::remove_file(fx.file("Lease.docx"));
    // Gone now: a file written at its place later is local.
    fx.probe.unmark(fx.file("Lease.docx"));
    let after = matter
        .after_turn(&meta(3), &tools(&["Lease.docx"]))
        .unwrap();
    assert_eq!(created(after, 3), [deleted("Lease.docx", b"the lease")]);
    let undone = undo(&mut matter, (1, 3), &["Lease.docx"], (4, 5)).unwrap();
    assert_eq!(undone.restored, [path("Lease.docx")]);
    assert_none(&undone.unversioned);
    assert_eq!(fx.read("Lease.docx"), b"the lease");
    assert_eq!(read_at(&matter, 5, "Lease.docx"), b"the lease");
}

/// A placeholder whose modification time is 40 ns off the one the cache read (iCloud Drive's
/// first-download rounding, seen on the Mac mini) is carried forward all the same.
#[test]
fn a_placeholder_40_ns_off_the_time_read_is_carried_forward() {
    let fx = Fixture::new("carry-40ns");
    old_file(&fx, "Lease.docx", b"the lease");
    let mut matter = fx.import();
    let moved = SystemTime::UNIX_EPOCH + PAST + Duration::from_nanos(40);
    crate::support::set_mtime(fx.file("Lease.docx"), moved);
    fx.probe.mark(fx.file("Lease.docx"));
    let opening = matter.before_turn(&meta(2)).unwrap();
    assert_none(&opening.skipped);
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

/// Another file put in the paper's place with its size and modification time (a copy given the
/// time with `touch -r`, then moved over it, as the Mac mini's row did), then evicted: a new
/// inode, so it is not carried forward. As before carrying forward, its content is unknown and
/// undo keeps the agent's later change. Unix only: on Windows the stat cache keeps no inode.
#[cfg(unix)]
#[test]
fn another_file_moved_into_its_place_with_its_size_and_time_is_not_carried_forward() {
    let fx = Fixture::new("carry-replaced");
    old_file(&fx, "Lease.docx", b"the lease");
    let mut matter = fx.import();
    let copy = fx.dir.path().join("copy.docx");
    crate::support::write(&copy, "THE LEASE");
    crate::support::set_mtime(&copy, SystemTime::UNIX_EPOCH + PAST);
    crate::support::rename(&copy, fx.file("Lease.docx"));
    fx.probe.mark(fx.file("Lease.docx"));
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
    assert_none(&undone.restored);
    assert_eq!(
        undone.unversioned,
        [SkippedFile {
            path: path("Lease.docx"),
            reason: SkippedFileReason::CloudOnly,
        }]
    );
    assert_eq!(fx.read("Lease.docx"), b"the lease, changed by the agent");
}

/// Answers cloud-only for `target` once armed, from the `nth` time it is asked about it on: a
/// file a sync client evicts between two looks at it.
struct EvictingProbe {
    target: PathBuf,
    nth: usize,
    armed: AtomicBool,
    seen: AtomicUsize,
}

impl PlaceholderProbe for EvictingProbe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        path == self.target
            && self.armed.load(Ordering::Relaxed)
            && self.seen.fetch_add(1, Ordering::Relaxed) + 1 >= self.nth
    }
}

/// Evicted after undo's first check that it may write it and before its outside-edits walk,
/// with its stat unchanged: the walk carries it forward as read, and undo still keeps it, as
/// it never writes or deletes a cloud-only file.
#[test]
fn a_paper_evicted_while_undo_runs_is_kept_and_not_written() {
    let fx = Fixture::new("carry-evicted-in-undo");
    // Asked about once by undo's first check, then by the outside-edits walk (evicted from here).
    let probe = Arc::new(EvictingProbe {
        target: fx.file("Lease.docx"),
        nth: 2,
        armed: AtomicBool::new(false),
        seen: AtomicUsize::new(0),
    });
    let settings = Settings {
        probe: Arc::<EvictingProbe>::clone(&probe),
        ..fx.settings()
    };
    old_file(&fx, "Lease.docx", b"the lease");
    let cancel = AtomicBool::new(false);
    let imported = Matter::import(settings, fx.new_matter(), &[], &mut |_| {}, &cancel);
    let Ok(Imported::Done { matter, .. }) = imported else {
        panic!("{imported:?}");
    };
    let mut matter = *matter;
    matter.before_turn(&meta(2)).unwrap();
    file_at(
        &fx,
        "Lease.docx",
        b"the lease, changed",
        PAST + Duration::from_secs(60),
    );
    let after = matter
        .after_turn(&meta(3), &tools(&["Lease.docx"]))
        .unwrap();
    assert_eq!(
        created(after, 3),
        [modified("Lease.docx", b"the lease", b"the lease, changed")]
    );

    probe.armed.store(true, Ordering::Relaxed);
    let undone = undo(&mut matter, (1, 3), &["Lease.docx"], (4, 5)).unwrap();
    assert_none(&undone.restored);
    assert_eq!(undone.kept, [path("Lease.docx")]);
    // Undo's first check, its outside-edits walk, the look again, the undo checkpoint's walk.
    assert_eq!(probe.seen.load(Ordering::Relaxed), 4);
    assert_eq!(fx.read("Lease.docx"), b"the lease, changed");
    assert_no_temp_files(&fx.folder);
}

/// Replacing a carried placeholder is refused for what it is, a file not read now (`NotVersioned`,
/// `word.edit_failed`), not as changed since it was read; nothing is written.
#[test]
fn replacing_a_carried_placeholder_is_refused_as_not_versioned() {
    let (fx, mut matter) = evicted("carry-replace");
    let work = fx.dir.path().join("work");
    crate::support::write(work.join("read.docx"), "the lease");
    crate::support::write(work.join("out.docx"), "the lease, revised");
    let file = path("Lease.docx");
    let (expected, new) = (work.join("read.docx"), work.join("out.docx"));
    let request = ReplaceFile {
        path: &file,
        expected: &expected,
        new: &new,
        outside_edits: cid(2),
        checkpoint: cid(3),
        at: at(3),
    };
    let error = matter.replace_file(&request).unwrap_err();
    assert!(
        matches!(&error, Error::NotVersioned { path } if *path == file),
        "{error}"
    );
    assert_eq!(error.code(), ErrorCode::WordEditFailed);
    assert_eq!(fx.read("Lease.docx"), b"the lease");
    assert_eq!(matter.timeline().len(), 1);
}
