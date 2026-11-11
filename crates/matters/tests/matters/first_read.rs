//! A file the last checkpoint skipped (cloud-only, or over the cap) that a turn reads for the
//! first time: it was in the folder before, so it is never an added paper and undo never
//! deletes or overwrites it. Read unchanged it is a baseline (no change, a `Clauseo-Baseline`
//! trailer); read changed it is listed, but undo keeps it (`Undone::unversioned`).

use std::path::Path;

use clauseo_matters::{Error, Limits};
use clauseo_protocol::{SkippedFile, SkippedFileReason};

use crate::fixture::{
    Fixture, added, assert_none, cid, created, meta, modified, path, read_at, tools, undo,
    undo_all, undo_request,
};

/// The message of the commit the timeline's ref (`refs/clauseo/timeline`) names.
fn tip_message(git_dir: &Path) -> String {
    use gix::objs::Find as _;
    let hex = std::fs::read_to_string(git_dir.join("refs/clauseo/timeline")).unwrap();
    let id = gix::ObjectId::from_hex(hex.trim().as_bytes()).unwrap();
    let odb = gix::odb::at(git_dir.join("objects"), gix::hash::Kind::Sha1).unwrap();
    let mut buffer = Vec::new();
    let data = odb.try_find(&id, &mut buffer).unwrap().unwrap();
    let commit = gix::objs::CommitRef::from_bytes(data.data, gix::hash::Kind::Sha1).unwrap();
    String::from_utf8(commit.message.to_vec()).unwrap()
}

/// The `Clauseo-Baseline` and `Clauseo-Was-Skipped` lines of the tip's message.
fn first_read_lines(fx: &Fixture) -> Vec<String> {
    tip_message(&fx.git_dir())
        .lines()
        .filter(|line| line.starts_with("Clauseo-Baseline: ") || line.starts_with("Clauseo-Was-"))
        .map(str::to_owned)
        .collect()
}

/// `rel` written and marked cloud-only, so the import skips it.
fn cloud_only(fx: &Fixture, rel: &str, bytes: &[u8]) {
    fx.write(rel, bytes);
    fx.probe.mark(fx.file(rel));
}

/// The `OneDrive` case: `notes.txt` is a placeholder Clauseo never read; the agent reads it, so
/// the OS downloads it (same bytes, same mtime), and edits another file. Only the edit is a
/// change; Undo puts it back and leaves `notes.txt` as it is.
#[test]
fn a_cloud_only_file_the_agent_only_read_is_a_baseline_and_undo_leaves_it() {
    let fx = Fixture::new("first-read-baseline");
    fx.write("brief.txt", "draft");
    cloud_only(&fx, "notes.txt", b"the lawyer's notes");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();

    fx.probe.unmark(fx.file("notes.txt"));
    fx.write("brief.txt", "final");
    let checkpoint = matter
        .after_turn(&meta(3), &tools(&["notes.txt", "brief.txt"]))
        .unwrap();
    assert_none(&checkpoint.skipped);
    assert_eq!(
        created(checkpoint, 3),
        [modified("brief.txt", b"draft", b"final")]
    );
    assert_eq!(
        first_read_lines(&fx),
        ["Clauseo-Baseline: cloud-only notes.txt"]
    );
    assert_eq!(read_at(&matter, 3, "notes.txt"), b"the lawyer's notes");
    assert_eq!(
        matter.changes_between(cid(1), cid(3)).unwrap(),
        [modified("brief.txt", b"draft", b"final")]
    );
    assert_none(&matter.edited_since(cid(1), cid(3)).unwrap());

    // A decision for the baseline is refused: it is not part of the step.
    let decisions = undo_all(&["brief.txt", "notes.txt"]);
    let refused = matter.undo(&undo_request(1, 3, &decisions, 4, 5));
    assert!(
        matches!(&refused, Err(Error::UnexpectedDecision { path: at }) if *at == path("notes.txt")),
        "{refused:?}"
    );
    let undone = undo(&mut matter, (1, 3), &["brief.txt"], (6, 7)).unwrap();
    assert_eq!(undone.restored, [path("brief.txt")]);
    assert_none(&undone.deleted);
    assert_none(&undone.unversioned);
    assert_eq!(fx.read("brief.txt"), b"draft");
    assert_eq!(fx.read("notes.txt"), b"the lawyer's notes");

    // Opened again, the history alone still says so.
    drop(matter);
    let matter = fx.open(&fx.folder).unwrap();
    assert_eq!(
        matter.changes_between(cid(1), cid(3)).unwrap(),
        [modified("brief.txt", b"draft", b"final")]
    );
}

/// A turn whose only "change" is a placeholder read is a checkpoint with no change: the
/// content is versioned from then on, and there is nothing to undo.
#[test]
fn a_turn_that_only_read_a_cloud_only_file_changed_nothing() {
    let fx = Fixture::new("first-read-only");
    fx.write("brief.txt", "draft");
    cloud_only(&fx, "notes.txt", b"the lawyer's notes");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.probe.unmark(fx.file("notes.txt"));
    let checkpoint = matter.after_turn(&meta(3), &tools(&["notes.txt"])).unwrap();
    assert_none(&created(checkpoint, 3));
    let message = tip_message(&fx.git_dir());
    assert!(message.starts_with("After turn: 0 files changed\n"));
    // Nothing skipped any more: the commit says so.
    assert!(message.contains("\nClauseo-Skips: none\n"), "{message}");
    assert_none(&matter.changes_between(cid(1), cid(3)).unwrap());
    let undone = undo(&mut matter, (1, 3), &[], (4, 5)).unwrap();
    assert_none(&undone.restored);
    assert_none(&undone.deleted);
    assert_eq!(fx.read("notes.txt"), b"the lawyer's notes");
}

/// A file over the cap, read once the cap is raised, unchanged: a baseline too.
#[test]
fn an_over_cap_file_read_after_the_cap_is_raised_is_a_baseline() {
    let mut fx = Fixture::new("first-read-over-cap");
    fx.limits = Limits {
        file_cap_bytes: 10,
        import_warn_bytes: 1_000_000,
    };
    fx.write("small.txt", "small");
    fx.write("big.bin", [7; 20]);
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    drop(matter);
    fx.limits.file_cap_bytes = 100;
    let mut matter = fx.open(&fx.folder).unwrap();
    let checkpoint = matter.after_turn(&meta(3), &tools(&[])).unwrap();
    assert_none(&created(checkpoint, 3));
    // The skip record says why it was skipped, whatever the cap is now.
    assert_eq!(
        first_read_lines(&fx),
        ["Clauseo-Baseline: over-size-cap big.bin"]
    );
    assert_eq!(read_at(&matter, 3, "big.bin"), [7; 20]);
    let undone = undo(&mut matter, (1, 3), &[], (4, 5)).unwrap();
    assert_none(&undone.deleted);
    assert_eq!(fx.read("big.bin"), [7; 20]);
}

/// A placeholder the agent downloaded and then edited, and an over-cap file it cut under the
/// cap, are changes, listed as added; but Clauseo had no copy from before, so Undo keeps each
/// byte for byte and says so, while a file the agent made is still deleted.
#[test]
fn a_skipped_file_the_agent_changed_is_listed_and_undo_keeps_it() {
    let mut fx = Fixture::new("first-read-changed");
    fx.limits = Limits {
        file_cap_bytes: 30,
        import_warn_bytes: 1_000_000,
    };
    cloud_only(&fx, "notes.txt", b"the lawyer's notes");
    cloud_only(&fx, "same-size.txt", b"twelve bytes");
    fx.write("big.bin", [7; 40]);
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();

    for rel in ["notes.txt", "same-size.txt"] {
        fx.probe.unmark(fx.file(rel));
    }
    fx.write("notes.txt", "the agent's notes, longer");
    // Same size, another mtime.
    fx.write("same-size.txt", "twelve BYTES");
    crate::support::settle(fx.file("same-size.txt"));
    fx.write("big.bin", "cut");
    fx.write("new.txt", "made by the agent");
    let all = ["big.bin", "new.txt", "notes.txt", "same-size.txt"];
    let checkpoint = matter.after_turn(&meta(3), &tools(&all)).unwrap();
    assert_eq!(
        created(checkpoint, 3),
        [
            added("big.bin", b"cut"),
            added("new.txt", b"made by the agent"),
            added("notes.txt", b"the agent's notes, longer"),
            added("same-size.txt", b"twelve BYTES"),
        ]
    );
    assert_eq!(
        first_read_lines(&fx),
        [
            "Clauseo-Was-Skipped: over-size-cap big.bin",
            "Clauseo-Was-Skipped: cloud-only notes.txt",
            "Clauseo-Was-Skipped: cloud-only same-size.txt",
        ]
    );
    // Undo would leave them as they are: none counts as edited since.
    assert_none(&matter.edited_since(cid(1), cid(3)).unwrap());

    let undone = undo(&mut matter, (1, 3), &all, (4, 5)).unwrap();
    assert_eq!(undone.restored, [path("new.txt")]);
    assert_eq!(undone.deleted, [path("new.txt")]);
    assert_eq!(
        undone.kept,
        [path("big.bin"), path("notes.txt"), path("same-size.txt")]
    );
    let unversioned = |rel: &str, reason| SkippedFile {
        path: path(rel),
        reason,
    };
    assert_eq!(
        undone.unversioned,
        [
            unversioned("big.bin", SkippedFileReason::OverSizeCap),
            unversioned("notes.txt", SkippedFileReason::CloudOnly),
            unversioned("same-size.txt", SkippedFileReason::CloudOnly),
        ]
    );
    assert!(!fx.exists("new.txt"));
    assert_eq!(fx.read("notes.txt"), b"the agent's notes, longer");
    assert_eq!(fx.read("same-size.txt"), b"twelve BYTES");
    assert_eq!(fx.read("big.bin"), b"cut");
}
