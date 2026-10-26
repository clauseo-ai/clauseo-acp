//! Files a checkpoint skips: over the size cap, and cloud-only placeholders, which are never
//! read. A skipped file is made unreadable (mode `0o000`) where the test can, so any read of it
//! fails the checkpoint.

use clauseo_matters::{CheckpointOutcome, Limits};
use clauseo_protocol::{SkippedFile, SkippedFileReason};

use crate::fixture::{
    Fixture, added, assert_none, cid, created, meta, modified, path, tools, undo,
};
#[cfg(unix)]
use crate::support;

fn skipped(at: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: path(at),
        reason,
    }
}

/// Makes `rel` unreadable, so a read of it fails.
fn lock_out(fx: &Fixture, rel: &str) {
    #[cfg(unix)]
    support::set_mode(fx.file(rel), 0o000);
    #[cfg(not(unix))]
    let _ = (fx, rel);
}

/// Makes `rel` readable again.
fn let_in(fx: &Fixture, rel: &str) {
    #[cfg(unix)]
    support::set_mode(fx.file(rel), 0o644);
    #[cfg(not(unix))]
    let _ = (fx, rel);
}

#[test]
fn over_cap_file_skipped_and_never_written() {
    let mut fx = Fixture::new("over-cap");
    fx.limits = Limits {
        file_cap_bytes: 10,
        import_warn_bytes: 1_000_000,
    };
    fx.write("small.txt", "small");
    fx.write("grow.txt", "grows");
    let mut matter = fx.import();

    // A new over-cap file is listed once, then not again while unchanged.
    fx.write("big.bin", [7; 20]);
    lock_out(&fx, "big.bin");
    let checkpoint = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_none(&checkpoint.changes);
    assert_eq!(
        checkpoint.skipped,
        [skipped("big.bin", SkippedFileReason::OverSizeCap)]
    );
    let checkpoint = matter.before_turn(&meta(3)).unwrap();
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_none(&checkpoint.skipped);

    // A versioned file that grows over the cap is skipped, not deleted.
    let grown = [8_u8; 30];
    fx.write("grow.txt", grown);
    fx.write("small.txt", "smaller");
    let checkpoint = matter.after_turn(&meta(4), &tools(&["small.txt"])).unwrap();
    assert_eq!(
        checkpoint.skipped,
        [skipped("grow.txt", SkippedFileReason::OverSizeCap)]
    );
    assert_eq!(
        created(checkpoint, 4),
        [modified("small.txt", b"small", b"smaller")]
    );

    // Undoing the step keeps the over-cap file as it is.
    let mtime = fx.file("grow.txt").metadata().unwrap().modified().unwrap();
    let undone = undo(&mut matter, (1, 4), &["small.txt"], (5, 6)).unwrap();
    assert_eq!(undone.restored, [path("small.txt")]);
    assert_eq!(fx.read("small.txt"), b"small");
    assert_eq!(fx.read("grow.txt"), grown);
    let after = fx.file("grow.txt").metadata().unwrap().modified().unwrap();
    assert_eq!(after, mtime);
    let_in(&fx, "big.bin");
    assert_eq!(fx.read("big.bin"), [7; 20]);
}

#[test]
fn cloud_only_file_skipped() {
    let fx = Fixture::new("cloud-skipped");
    fx.write("a.txt", "a");
    fx.write("cloud.docx", "in the cloud");
    fx.probe.mark(fx.file("cloud.docx"));
    lock_out(&fx, "cloud.docx");
    let (mut matter, summary) = fx.import_with_summary();
    assert_eq!((summary.files, summary.cloud_only), (1, 1));

    fx.write("cloud2.docx", "also in the cloud");
    fx.probe.mark(fx.file("cloud2.docx"));
    lock_out(&fx, "cloud2.docx");
    let checkpoint = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_eq!(
        checkpoint.skipped,
        [skipped("cloud2.docx", SkippedFileReason::CloudOnly)]
    );
    let checkpoint = matter.before_turn(&meta(3)).unwrap();
    assert_none(&checkpoint.skipped);

    // Downloaded: now it is versioned, as a new file.
    let_in(&fx, "cloud2.docx");
    fx.probe.unmark(fx.file("cloud2.docx"));
    let changes = created(matter.before_turn(&meta(4)).unwrap(), 4);
    assert_eq!(changes, [added("cloud2.docx", b"also in the cloud")]);
}

#[test]
fn cloud_only_file_keeps_previous_version() {
    let fx = Fixture::new("cloud-keeps");
    fx.write("doc.docx", "v1");
    let mut matter = fx.import();

    fx.write("doc.docx", "v2, changed while cloud-only");
    fx.probe.mark(fx.file("doc.docx"));
    lock_out(&fx, "doc.docx");
    let checkpoint = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(checkpoint.outcome, CheckpointOutcome::Reused(cid(1)));
    assert_none(&checkpoint.changes);
    assert_eq!(
        checkpoint.skipped,
        [skipped("doc.docx", SkippedFileReason::CloudOnly)]
    );

    let_in(&fx, "doc.docx");
    fx.probe.unmark(fx.file("doc.docx"));
    let changes = created(matter.before_turn(&meta(3)).unwrap(), 3);
    assert_eq!(
        changes,
        [modified("doc.docx", b"v1", b"v2, changed while cloud-only")]
    );
}

#[test]
fn changed_cloud_only_file_never_read() {
    let fx = Fixture::new("cloud-never-read");
    fx.write("doc.docx", "v1");
    fx.write("other.txt", "other v1");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();

    // During the turn the document changes and goes cloud-only; a new cloud-only file appears.
    fx.write("doc.docx", "v2 from the cloud");
    fx.write("other.txt", "other v2");
    fx.write("new.docx", "never versioned");
    for rel in ["doc.docx", "new.docx"] {
        fx.probe.mark(fx.file(rel));
        lock_out(&fx, rel);
    }
    let tool_paths = tools(&["doc.docx", "other.txt", "new.docx"]);
    let checkpoint = matter.after_turn(&meta(3), &tool_paths).unwrap();
    assert_eq!(
        checkpoint.skipped,
        [
            skipped("doc.docx", SkippedFileReason::CloudOnly),
            skipped("new.docx", SkippedFileReason::CloudOnly),
        ]
    );
    assert_eq!(
        created(checkpoint, 3),
        [modified("other.txt", b"other v1", b"other v2")]
    );

    // The step's tree holds the old document: undo sees only other.txt and leaves doc alone.
    assert_none(&matter.edited_since(cid(1), cid(3)).unwrap());
    let undone = undo(&mut matter, (1, 3), &["other.txt"], (4, 5)).unwrap();
    assert_eq!(undone.restored, [path("other.txt")]);
    assert_eq!(
        created(undone.checkpoint, 5),
        [modified("other.txt", b"other v2", b"other v1")]
    );

    // Downloaded again: the change is from the old content id, and the new file is added.
    for rel in ["doc.docx", "new.docx"] {
        let_in(&fx, rel);
        fx.probe.unmark(fx.file(rel));
    }
    assert_eq!(fx.read("doc.docx"), b"v2 from the cloud");
    let changes = created(matter.before_turn(&meta(6)).unwrap(), 6);
    assert_eq!(
        changes,
        [
            modified("doc.docx", b"v1", b"v2 from the cloud"),
            added("new.docx", b"never versioned"),
        ]
    );
}

/// A subfolder that does not read is skipped with a warning and its files stay in the tree as
/// they were: neither deleted nor changed, and the rest of the folder is checkpointed.
#[cfg(unix)]
#[test]
fn unreadable_subfolder_keeps_its_files() {
    let fx = Fixture::new("unreadable-subfolder");
    fx.write("locked/inner.txt", "inside");
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    support::set_mode(fx.file("locked"), 0o000);
    if std::fs::read_dir(fx.file("locked")).is_ok() {
        // Running as root, which reads any folder: nothing to check.
        support::set_mode(fx.file("locked"), 0o755);
        eprintln!("skipped: this user reads a mode-000 folder");
        return;
    }
    fx.write("a.txt", "two");
    let checkpoint = matter.before_turn(&meta(2));
    support::set_mode(fx.file("locked"), 0o755);
    assert_eq!(
        created(checkpoint.unwrap(), 2),
        [modified("a.txt", b"one", b"two")]
    );
    // Readable again and unchanged: nothing to record.
    let again = matter.before_turn(&meta(3)).unwrap();
    assert_eq!(again.outcome, CheckpointOutcome::Reused(cid(2)));
}
