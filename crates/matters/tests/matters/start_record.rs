//! What was in the folder at a turn's start, as the turn's first checkpoint records it (its
//! `Clauseo-Skips` record), so Undo deletes only a file known to have been absent then: never
//! one in a folder that did not list, one moved from a skipped file, or one the stat cache
//! forgot because it was lost or rebuilt.

use clauseo_matters::CompareSide::{Checkpoint, Folder};
use clauseo_protocol::{SkippedFile, SkippedFileReason};

use crate::fixture::{
    Fixture, added, assert_none, cid, created, meta, modified, path, tools, undo,
};
use crate::support;

/// `rel` written and marked cloud-only, so a walk skips it.
fn cloud_only(fx: &Fixture, rel: &str, bytes: &[u8]) {
    fx.write(rel, bytes);
    fx.probe.mark(fx.file(rel));
}

fn kept(rel: &str, reason: SkippedFileReason) -> SkippedFile {
    SkippedFile {
        path: path(rel),
        reason,
    }
}

/// A folder that does not list at the turn's start (permission denied) and does after: its
/// file reads as added, but Undo keeps it; and since a file may have come out of that folder,
/// so does the one the agent made.
#[cfg(unix)]
#[test]
fn a_file_in_a_folder_unread_at_the_start_is_kept_by_undo() {
    let fx = Fixture::new("start-unread-folder");
    fx.write("a.txt", "one");
    fx.write("Sub/lawyer.txt", "the lawyer's file");
    support::set_mode(fx.file("Sub"), 0o000);
    if std::fs::read_dir(fx.file("Sub")).is_ok() {
        // Running as root, which reads any folder: nothing to check.
        support::set_mode(fx.file("Sub"), 0o755);
        eprintln!("skipped: this user reads a mode-000 folder");
        return;
    }
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    support::set_mode(fx.file("Sub"), 0o755);
    fx.write("new.txt", "made by the agent");
    let checkpoint = matter.after_turn(&meta(3), &tools(&["new.txt"])).unwrap();
    let changes = created(checkpoint, 3);
    let paths: Vec<&str> = changes
        .iter()
        .map(|change| change.path().as_str())
        .collect();
    assert_eq!(paths, ["Sub/lawyer.txt", "new.txt"]);

    let undone = undo(&mut matter, (1, 3), &["Sub/lawyer.txt", "new.txt"], (4, 5)).unwrap();
    assert_none(&undone.deleted);
    assert_eq!(
        undone.unversioned,
        [
            kept("Sub/lawyer.txt", SkippedFileReason::UnreadFolder),
            kept("new.txt", SkippedFileReason::UnreadFolder),
        ]
    );
    assert_eq!(fx.read("Sub/lawyer.txt"), b"the lawyer's file");
    assert_eq!(fx.read("new.txt"), b"made by the agent");
}

/// Makes `Sub` (holding the lawyer's file) unreadable, for a turn to start with it unread;
/// `None` when this user reads a mode-000 folder anyway (root), so there is nothing to check.
#[cfg(unix)]
fn unread_sub(name: &str) -> Option<Fixture> {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    fx.write("Sub/lawyer.txt", "the lawyer's file");
    support::set_mode(fx.file("Sub"), 0o000);
    if std::fs::read_dir(fx.file("Sub")).is_ok() {
        support::set_mode(fx.file("Sub"), 0o755);
        eprintln!("skipped: this user reads a mode-000 folder");
        return None;
    }
    Some(fx)
}

/// The agent moves the lawyer's file out of a folder that did not list at the turn's start (it
/// lists by the end) to a name never seen before: Undo keeps it, byte for byte.
#[cfg(unix)]
#[test]
fn a_file_moved_out_of_a_folder_unread_at_the_start_is_kept_by_undo() {
    let Some(fx) = unread_sub("start-moved-out-of-unread") else {
        return;
    };
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    support::set_mode(fx.file("Sub"), 0o755);
    support::rename(fx.file("Sub/lawyer.txt"), fx.file("lawyer.txt"));
    let checkpoint = matter
        .after_turn(&meta(3), &tools(&["lawyer.txt"]))
        .unwrap();
    assert_eq!(
        created(checkpoint, 3),
        [added("lawyer.txt", b"the lawyer's file")]
    );
    let undone = undo(&mut matter, (1, 3), &["lawyer.txt"], (4, 5)).unwrap();
    assert_none(&undone.deleted);
    assert_eq!(
        undone.unversioned,
        [kept("lawyer.txt", SkippedFileReason::UnreadFolder)]
    );
    assert_eq!(fx.read("lawyer.txt"), b"the lawyer's file");
}

/// A folder that did not list at the start and still does not at the end: nothing can have come
/// out of it, so a file the agent made elsewhere is still deleted.
#[cfg(unix)]
#[test]
fn a_folder_still_unread_at_the_end_keeps_the_normal_rule() {
    let Some(fx) = unread_sub("start-still-unread") else {
        return;
    };
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("new.txt", "made by the agent");
    let checkpoint = matter.after_turn(&meta(3), &tools(&["new.txt"])).unwrap();
    assert_eq!(
        created(checkpoint, 3),
        [added("new.txt", b"made by the agent")]
    );
    let undone = undo(&mut matter, (1, 3), &["new.txt"], (4, 5));
    support::set_mode(fx.file("Sub"), 0o755);
    let undone = undone.unwrap();
    assert_eq!(undone.deleted, [path("new.txt")]);
    assert_none(&undone.unversioned);
    assert!(!fx.exists("new.txt"));
    assert_eq!(fx.read("Sub/lawyer.txt"), b"the lawyer's file");
}

/// A folder that did not list at the start is gone by the end (the agent moved it whole): its
/// files, under the new name, and any other file the turn added are kept.
#[cfg(unix)]
#[test]
fn a_folder_unread_at_the_start_and_gone_at_the_end_keeps_every_added_file() {
    let Some(fx) = unread_sub("start-unread-gone") else {
        return;
    };
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    support::rename(fx.file("Sub"), fx.file("Moved"));
    support::set_mode(fx.file("Moved"), 0o755);
    fx.write("new.txt", "made by the agent");
    let checkpoint = matter
        .after_turn(&meta(3), &tools(&["Moved/lawyer.txt", "new.txt"]))
        .unwrap();
    let changes = created(checkpoint, 3);
    let paths: Vec<&str> = changes
        .iter()
        .map(|change| change.path().as_str())
        .collect();
    assert_eq!(paths, ["Moved/lawyer.txt", "new.txt"]);
    let undone = undo(
        &mut matter,
        (1, 3),
        &["Moved/lawyer.txt", "new.txt"],
        (4, 5),
    )
    .unwrap();
    assert_none(&undone.deleted);
    assert_eq!(
        undone.unversioned,
        [
            kept("Moved/lawyer.txt", SkippedFileReason::UnreadFolder),
            kept("new.txt", SkippedFileReason::UnreadFolder),
        ]
    );
    assert_eq!(fx.read("Moved/lawyer.txt"), b"the lawyer's file");
    assert_eq!(fx.read("new.txt"), b"made by the agent");
}

/// A placeholder the agent downloaded, then moved: the new path is that file, the only copy,
/// so Undo keeps it.
#[test]
fn a_cloud_only_file_read_then_moved_is_kept_by_undo() {
    let fx = Fixture::new("start-moved-placeholder");
    fx.write("brief.txt", "draft");
    cloud_only(&fx, "notes.txt", b"the lawyer's notes");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.probe.unmark(fx.file("notes.txt"));
    support::create_dir_all(fx.file("Archive"));
    support::rename(fx.file("notes.txt"), fx.file("Archive/notes.txt"));
    let tool_paths = tools(&["notes.txt", "Archive/notes.txt"]);
    let checkpoint = matter.after_turn(&meta(3), &tool_paths).unwrap();
    assert_eq!(
        created(checkpoint, 3),
        [added("Archive/notes.txt", b"the lawyer's notes")]
    );
    let undone = undo(&mut matter, (1, 3), &["Archive/notes.txt"], (4, 5)).unwrap();
    assert_none(&undone.deleted);
    assert_eq!(
        undone.unversioned,
        [kept("Archive/notes.txt", SkippedFileReason::CloudOnly)]
    );
    assert_eq!(fx.read("Archive/notes.txt"), b"the lawyer's notes");
}

/// The stat cache lost between the turn's start and its end (the matter opened again with no
/// `history.git/index`): the record still says the placeholder was there, so it is a baseline
/// and Undo leaves it; the file the agent made is still deleted.
#[test]
fn a_lost_stat_cache_forgets_no_skipped_file() {
    let fx = Fixture::new("start-lost-cache");
    fx.write("brief.txt", "draft");
    cloud_only(&fx, "notes.txt", b"the lawyer's notes");
    let matter = fx.import();
    drop(matter);
    let mut matter = fx.open(&fx.folder).unwrap();
    matter.before_turn(&meta(2)).unwrap();
    drop(matter);
    support::remove_file(fx.git_dir().join("index"));
    let mut matter = fx.open(&fx.folder).unwrap();
    fx.probe.unmark(fx.file("notes.txt"));
    fx.write("new.txt", "made by the agent");
    let checkpoint = matter.after_turn(&meta(3), &tools(&["new.txt"])).unwrap();
    assert_eq!(
        created(checkpoint, 3),
        [added("new.txt", b"made by the agent")]
    );
    let undone = undo(&mut matter, (1, 3), &["new.txt"], (4, 5)).unwrap();
    assert_eq!(undone.deleted, [path("new.txt")]);
    assert_eq!(fx.read("notes.txt"), b"the lawyer's notes");
}

/// A stat cache rebuilt from the tip's tree (here: one left from an older tree, put back) has
/// no skipped files; the record still has them.
#[test]
fn a_rebuilt_stat_cache_forgets_no_skipped_file() {
    let fx = Fixture::new("start-rebuilt-cache");
    fx.write("brief.txt", "draft");
    cloud_only(&fx, "notes.txt", b"the lawyer's notes");
    let mut matter = fx.import();
    let index = fx.git_dir().join("index");
    let stale = std::fs::read(&index).unwrap();
    fx.write("brief.txt", "second draft");
    created(matter.before_turn(&meta(2)).unwrap(), 2);
    drop(matter);
    // The cache of checkpoint 1 under checkpoint 2's tree: rebuilt from that tree on open.
    support::write(&index, &stale);
    let mut matter = fx.open(&fx.folder).unwrap();
    fx.probe.unmark(fx.file("notes.txt"));
    fx.write("brief.txt", "final");
    fx.write("new.txt", "made by the agent");
    let checkpoint = matter
        .after_turn(&meta(3), &tools(&["brief.txt", "new.txt"]))
        .unwrap();
    let changes = created(checkpoint, 3);
    let paths: Vec<&str> = changes
        .iter()
        .map(|change| change.path().as_str())
        .collect();
    assert_eq!(paths, ["brief.txt", "new.txt"]);
    let undone = undo(&mut matter, (2, 3), &["brief.txt", "new.txt"], (4, 5)).unwrap();
    assert_eq!(undone.deleted, [path("new.txt")]);
    assert_eq!(fx.read("brief.txt"), b"second draft");
    assert_eq!(fx.read("notes.txt"), b"the lawyer's notes");
}

/// A versioned file that went cloud-only, which a colleague then changed in the cloud: at the
/// turn's start its content is unknown. Read by the agent with the size and mtime it had then, it
/// is a baseline, no change; edited by the agent, Undo never writes the old version over it.
#[test]
fn a_cloud_only_file_a_colleague_changed_is_never_overwritten_by_undo() {
    let fx = Fixture::new("start-stale-file");
    fx.write("lease.docx", "v1");
    fx.write("brief.txt", "draft");
    let mut matter = fx.import();
    fx.probe.mark(fx.file("lease.docx"));
    fx.write("lease.docx", "v2, the colleague's");
    assert_none(&created(matter.before_turn(&meta(2)).unwrap(), 2));

    // The agent only reads it: its content is the colleague's, and nothing changed.
    fx.probe.unmark(fx.file("lease.docx"));
    assert_none(&created(
        matter
            .after_turn(&meta(3), &tools(&["lease.docx"]))
            .unwrap(),
        3,
    ));
    let undone = undo(&mut matter, (2, 3), &[], (4, 5)).unwrap();
    assert_none(&undone.restored);
    assert_none(&undone.deleted);
    assert_eq!(fx.read("lease.docx"), b"v2, the colleague's");

    // Cloud-only again, and changed again in the cloud; this time the agent edits it too.
    fx.probe.mark(fx.file("lease.docx"));
    fx.write("lease.docx", "v3, the colleague's again");
    created(matter.before_turn(&meta(6)).unwrap(), 6);
    fx.probe.unmark(fx.file("lease.docx"));
    fx.write("lease.docx", "v4, the agent's");
    fx.write("brief.txt", "final");
    let checkpoint = matter
        .after_turn(&meta(7), &tools(&["lease.docx", "brief.txt"]))
        .unwrap();
    assert_eq!(
        created(checkpoint, 7),
        [
            modified("brief.txt", b"draft", b"final"),
            modified("lease.docx", b"v2, the colleague's", b"v4, the agent's"),
        ]
    );
    let undone = undo(&mut matter, (6, 7), &["brief.txt", "lease.docx"], (8, 9)).unwrap();
    assert_eq!(undone.restored, [path("brief.txt")]);
    assert_eq!(
        undone.unversioned,
        [kept("lease.docx", SkippedFileReason::CloudOnly)]
    );
    assert_eq!(fx.read("lease.docx"), b"v4, the agent's");
    assert_eq!(fx.read("brief.txt"), b"draft");
}

/// A baseline is no change in any comparison: either way round, across several turns, and
/// against the folder now (where one read but not checkpointed yet is none either).
#[test]
fn a_baseline_is_no_change_in_any_comparison() {
    let fx = Fixture::new("start-compare");
    fx.write("brief.txt", "draft");
    cloud_only(&fx, "notes.txt", b"the lawyer's notes");
    cloud_only(&fx, "later.txt", b"read later");
    let mut matter = fx.import();
    fx.probe.unmark(fx.file("notes.txt"));
    fx.write("brief.txt", "second");
    created(
        matter.after_turn(&meta(2), &tools(&["brief.txt"])).unwrap(),
        2,
    );
    fx.write("brief.txt", "third");
    created(
        matter.after_turn(&meta(3), &tools(&["brief.txt"])).unwrap(),
        3,
    );
    let brief = |old: &[u8], new: &[u8]| vec![modified("brief.txt", old, new)];
    assert_eq!(
        matter
            .compare(Checkpoint(cid(1)), Checkpoint(cid(3)))
            .unwrap(),
        brief(b"draft", b"third")
    );
    assert_eq!(
        matter
            .compare(Checkpoint(cid(3)), Checkpoint(cid(1)))
            .unwrap(),
        brief(b"third", b"draft")
    );
    assert_eq!(
        matter
            .compare(Checkpoint(cid(2)), Checkpoint(cid(1)))
            .unwrap(),
        brief(b"second", b"draft")
    );
    // later.txt downloaded, unchanged, not checkpointed yet.
    fx.probe.unmark(fx.file("later.txt"));
    assert_eq!(
        matter.compare(Checkpoint(cid(1)), Folder).unwrap(),
        brief(b"draft", b"third")
    );
    assert_eq!(
        matter.compare(Folder, Checkpoint(cid(1))).unwrap(),
        brief(b"third", b"draft")
    );
}

/// A versioned file a colleague changed in the cloud (cloud-only, not read, at the turn's start)
/// that the agent downloads and moves: the new path holds the colleague's version, the only copy,
/// so Undo keeps it, and never writes the old version back at the old path either.
#[test]
fn a_cloud_only_file_a_colleague_changed_then_moved_is_kept_by_undo() {
    let fx = Fixture::new("start-stale-moved");
    fx.write("lease.txt", "v1");
    let mut matter = fx.import();
    fx.probe.mark(fx.file("lease.txt"));
    fx.write("lease.txt", "v2, the colleague's");
    assert_none(&created(matter.before_turn(&meta(2)).unwrap(), 2));
    fx.probe.unmark(fx.file("lease.txt"));
    support::create_dir_all(fx.file("Archive"));
    support::rename(fx.file("lease.txt"), fx.file("Archive/lease.txt"));
    let tool_paths = tools(&["lease.txt", "Archive/lease.txt"]);
    let changes = created(matter.after_turn(&meta(3), &tool_paths).unwrap(), 3);
    let paths: Vec<&str> = changes
        .iter()
        .map(|change| change.path().as_str())
        .collect();
    assert_eq!(paths, ["Archive/lease.txt", "lease.txt"]);
    let undone = undo(
        &mut matter,
        (2, 3),
        &["Archive/lease.txt", "lease.txt"],
        (4, 5),
    )
    .unwrap();
    assert_none(&undone.deleted);
    assert_none(&undone.restored);
    assert_eq!(
        undone.unversioned,
        [
            kept("Archive/lease.txt", SkippedFileReason::CloudOnly),
            kept("lease.txt", SkippedFileReason::CloudOnly),
        ]
    );
    assert_eq!(fx.read("Archive/lease.txt"), b"v2, the colleague's");
    assert!(!fx.exists("lease.txt"));
}
