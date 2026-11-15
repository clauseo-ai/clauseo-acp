//! Files gone with no copy: a file the before-turn checkpoint skipped (cloud-only, or over the
//! size cap) that the after-turn checkpoint no longer finds is listed in `Checkpoint::gone` with
//! why it was skipped. Its deletion is no change (no version of it exists), so without the list
//! it would vanish with nothing said. A file the turn made and deleted, one moved, or one under a
//! folder that stopped listing is not gone.

use clauseo_matters::Limits;
use clauseo_protocol::{SkippedFile, SkippedFileReason};

use crate::fixture::{Fixture, assert_none, created, meta, path, tools};
use crate::support;

fn gone(at: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: path(at),
        reason,
    }
}

/// A fixture whose size cap is 10 bytes.
fn small_cap(name: &str) -> Fixture {
    let mut fx = Fixture::new(name);
    fx.limits = Limits {
        file_cap_bytes: 10,
        import_warn_bytes: 1_000_000,
    };
    fx
}

#[test]
fn a_cloud_only_file_and_an_over_cap_file_deleted_by_the_turn_are_gone_with_their_reasons() {
    let fx = small_cap("gone-both");
    fx.write("brief.txt", "draft");
    fx.write("Scans/photos.zip", [7; 20]);
    fx.write("Drafts/Lease.docx", "lease");
    fx.probe.mark(fx.file("Drafts/Lease.docx"));
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();

    // The agent's shell deletes both; the tools named nothing.
    support::remove_file(fx.file("Scans/photos.zip"));
    support::remove_file(fx.file("Drafts/Lease.docx"));
    let checkpoint = matter.after_turn(&meta(3), &tools(&[])).unwrap();
    assert_eq!(
        checkpoint.gone,
        [
            gone("Drafts/Lease.docx", SkippedFileReason::CloudOnly),
            gone("Scans/photos.zip", SkippedFileReason::OverSizeCap),
        ]
    );
    // No change: nothing for an undo to put back.
    assert_none(&checkpoint.skipped);
    assert_none(&created(checkpoint, 3));
}

#[test]
fn a_file_the_turn_made_and_deleted_or_a_kept_skipped_file_is_not_gone() {
    let fx = small_cap("gone-none");
    fx.write("brief.txt", "draft");
    fx.write("big.bin", [7; 20]);
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("scratch.bin", [1; 30]);
    support::remove_file(fx.file("scratch.bin"));
    fx.write("brief.txt", "final");
    let checkpoint = matter.after_turn(&meta(3), &tools(&[])).unwrap();
    assert_none(&checkpoint.gone);
}

#[test]
fn a_skipped_file_moved_by_the_turn_is_not_gone() {
    let fx = small_cap("gone-moved");
    fx.write("brief.txt", "draft");
    fx.write("big.bin", [7; 20]);
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    support::create_dir_all(fx.file("Archive"));
    support::rename(fx.file("big.bin"), fx.file("Archive/big.bin"));
    let checkpoint = matter.after_turn(&meta(3), &tools(&[])).unwrap();
    assert_none(&checkpoint.gone);
}

#[test]
fn a_skipped_file_deleted_between_turns_is_the_before_turn_checkpoints_not_the_turns() {
    let fx = small_cap("gone-between");
    fx.write("brief.txt", "draft");
    fx.write("big.bin", [7; 20]);
    let mut matter = fx.import();
    support::remove_file(fx.file("big.bin"));
    let before = matter.before_turn(&meta(2)).unwrap();
    assert_eq!(
        before.gone,
        [gone("big.bin", SkippedFileReason::OverSizeCap)]
    );
    let after = matter.after_turn(&meta(3), &tools(&[])).unwrap();
    assert_none(&after.gone);
}
