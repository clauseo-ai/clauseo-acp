//! Restoring a version: the whole matter or chosen files, forward only; files added since stay;
//! the outside-edits checkpoint; locks, links, skipped files and names spelled otherwise; the
//! preview; and every refusal before anything is written.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use clauseo_matters::{
    CheckpointOutcome, Error, Imported, Matter, RestoreFiles, RestorePlan, Settings, TimelineEntry,
};
use clauseo_protocol::{CheckpointKind, ErrorCode, MatterPath};

use crate::fixture::{
    Fixture, added, assert_no_temp_files, assert_none, cid, created, deleted, meta, modified, path,
    read_at, restore_request, tools, undo,
};
use crate::support;
use crate::undo::TouchingProbe;
use crate::undo_case::{CASE_REQUIRED, insensitive, names};

/// Checkpoint 1 holds `a.txt` "one" and `b.txt` "bee"; checkpoint 3, after a turn, holds
/// `a.txt` "two" and `d.txt` "dee" (`b.txt` deleted).
fn versioned(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    fx.write("b.txt", "bee");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    support::remove_file(fx.file("b.txt"));
    fx.write("d.txt", "dee");
    created(
        matter
            .after_turn(&meta(3), &tools(&["a.txt", "b.txt", "d.txt"]))
            .unwrap(),
        3,
    );
    (fx, matter)
}

fn paths(texts: &[&str]) -> Vec<MatterPath> {
    texts.iter().map(|text| path(text)).collect()
}

/// Every file under `dir`, recursively: what a preview must leave as it is.
fn count_files(dir: &Path) -> usize {
    let mut count = 0;
    let mut stack = vec![dir.to_owned()];
    while let Some(at) = stack.pop() {
        for entry in std::fs::read_dir(&at).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                stack.push(entry.path());
            } else {
                count += 1;
            }
        }
    }
    count
}

#[test]
fn restore_matter_and_single_file_forward_only() {
    let (fx, mut matter) = versioned("restore-forward");

    let preview = matter.preview_restore(cid(1), RestoreFiles::All).unwrap();
    assert_eq!(
        preview,
        RestorePlan {
            writes: vec![modified("a.txt", b"two", b"one"), added("b.txt", b"bee")],
            added_since: paths(&["d.txt"]),
            kept: Vec::new(),
            locked: Vec::new(),
        }
    );

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_eq!(restored.outside_edits, None);
    assert_eq!(restored.written, paths(&["a.txt", "b.txt"]));
    assert_eq!(restored.added_since, paths(&["d.txt"]));
    assert_none(&restored.kept);
    assert_eq!(
        created(restored.checkpoint, 5),
        [modified("a.txt", b"two", b"one"), added("b.txt", b"bee")]
    );
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(fx.read("b.txt"), b"bee");
    assert_eq!(fx.read("d.txt"), b"dee", "files added since stay");
    assert_no_temp_files(&fx.folder);
    let ids: Vec<_> = matter.timeline().iter().map(|entry| entry.id).collect();
    assert_eq!(
        ids,
        [cid(1), cid(3), cid(5)],
        "nothing is removed from the timeline"
    );
    assert_eq!(
        matter.timeline().last(),
        Some(&TimelineEntry {
            id: cid(5),
            kind: CheckpointKind::Restore,
            thread: None,
            turn: None,
        })
    );

    // One file, from the later version: only that file changes.
    let chosen = paths(&["a.txt"]);
    let restored = matter
        .restore(&restore_request(3, RestoreFiles::Only(&chosen), 6, 7))
        .unwrap();
    assert_eq!(restored.written, chosen);
    assert_none(&restored.added_since);
    assert_eq!(
        created(restored.checkpoint, 7),
        [modified("a.txt", b"one", b"two")]
    );
    assert_eq!(fx.read("a.txt"), b"two");
    assert_eq!(fx.read("b.txt"), b"bee");

    // A restore is a step like any other: undoing it puts the file back.
    let undone = undo(&mut matter, (5, 7), &["a.txt"], (8, 9)).unwrap();
    assert_eq!(undone.restored, chosen);
    assert_eq!(fx.read("a.txt"), b"one");
}

#[test]
fn restore_refuses_locked_file_before_writing() {
    let (fx, mut matter) = versioned("restore-locked");
    fx.write("~$a.txt", "owner file: open in Word");
    fx.write("d.txt", "an outside edit");

    let preview = matter.preview_restore(cid(1), RestoreFiles::All).unwrap();
    assert_eq!(preview.locked, paths(&["a.txt"]));

    let err = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap_err();
    assert!(
        matches!(&err, Error::FileLocked { paths } if *paths == [path("a.txt")]),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::MatterFileLocked);
    assert_eq!(fx.read("a.txt"), b"two");
    assert!(!fx.exists("b.txt"));
    assert_eq!(
        matter.timeline().len(),
        2,
        "no outside-edits checkpoint either"
    );
    assert_no_temp_files(&fx.folder);

    // Closed in Word: the restore goes through, after the outside edit is kept.
    support::remove_file(fx.file("~$a.txt"));
    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_eq!(
        created(restored.outside_edits.unwrap(), 4),
        [modified("d.txt", b"dee", b"an outside edit")]
    );
    assert_eq!(fx.read("a.txt"), b"one");
}

#[cfg(unix)]
#[test]
fn restore_never_writes_through_symlink() {
    let fx = Fixture::new("restore-symlink");
    fx.write("a.txt", "one");
    fx.write("sub/c.txt", "sea");
    let mut matter = fx.import();
    // The folder is moved out and a link to it put in its place.
    let elsewhere = fx.dir.path().join("elsewhere");
    support::rename(fx.file("sub"), &elsewhere);
    support::write(elsewhere.join("c.txt"), "outside the matter");
    support::symlink(&elsewhere, fx.file("sub"));
    fx.write("a.txt", "two");

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_eq!(restored.written, paths(&["a.txt"]));
    assert_eq!(restored.kept, paths(&["sub/c.txt"]));
    assert_eq!(
        std::fs::read(elsewhere.join("c.txt")).unwrap(),
        b"outside the matter"
    );
    assert_eq!(fx.read("a.txt"), b"one");
    assert_no_temp_files(&elsewhere);
}

#[test]
fn restore_takes_outside_edits_checkpoint_first() {
    let (fx, mut matter) = versioned("restore-outside");
    fx.write("a.txt", "the lawyer's edit");

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    let outside = restored.outside_edits.expect("an outside-edits checkpoint");
    assert_eq!(
        created(outside, 4),
        [modified("a.txt", b"two", b"the lawyer's edit")]
    );
    assert_eq!(
        matter.timeline()[2],
        TimelineEntry {
            id: cid(4),
            kind: CheckpointKind::OutsideEdits,
            thread: None,
            turn: None,
        }
    );
    assert_eq!(fx.read("a.txt"), b"one");
    // The overwritten edit is still in Versions.
    assert_eq!(read_at(&matter, 4, "a.txt"), b"the lawyer's edit");
}

#[test]
fn restore_keeps_files_added_since_and_renamed() {
    let fx = Fixture::new("restore-renamed");
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    support::rename(fx.file("a.txt"), fx.file("b.txt"));
    fx.write("new.txt", "new");

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_eq!(restored.written, paths(&["a.txt"]));
    assert_eq!(restored.added_since, paths(&["b.txt", "new.txt"]));
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(fx.read("b.txt"), b"one");
    assert_eq!(fx.read("new.txt"), b"new");
    assert_eq!(created(restored.checkpoint, 5), [added("a.txt", b"one")]);
}

#[test]
fn restore_of_current_version_reuses() {
    let (fx, mut matter) = versioned("restore-reuse");
    let preview = matter.preview_restore(cid(3), RestoreFiles::All).unwrap();
    assert_none(&preview.writes);

    let restored = matter
        .restore(&restore_request(3, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_eq!(restored.outside_edits, None);
    assert_eq!(
        restored.checkpoint.outcome,
        CheckpointOutcome::Reused(cid(3))
    );
    assert_none(&restored.checkpoint.changes);
    assert_none(&restored.written);
    assert_eq!(matter.timeline().len(), 2);
    assert_eq!(fx.read("a.txt"), b"two");
    assert_no_temp_files(&fx.folder);
}

#[test]
fn restore_chosen_file_added_since_stays() {
    let (fx, mut matter) = versioned("restore-chosen-added");
    let chosen = paths(&["d.txt"]);
    let restored = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 4, 5))
        .unwrap();
    assert_none(&restored.written);
    assert_eq!(restored.added_since, chosen);
    assert_eq!(
        restored.checkpoint.outcome,
        CheckpointOutcome::Reused(cid(3))
    );
    assert_eq!(fx.read("d.txt"), b"dee");
    assert_eq!(fx.read("a.txt"), b"two");
}

#[test]
fn restore_refuses_bad_requests_before_writing() {
    let (fx, mut matter) = versioned("restore-refused");
    fx.write("a.txt", "an outside edit");
    let unknown = paths(&["a.txt", "nowhere.txt"]);
    let twice = paths(&["a.txt", "a.txt"]);
    let cases: [(clauseo_matters::Restore<'_>, &str); 6] = [
        (
            restore_request(1, RestoreFiles::Only(&unknown), 4, 5),
            "UnknownFile",
        ),
        (
            restore_request(1, RestoreFiles::Only(&twice), 4, 5),
            "UnknownFile",
        ),
        (
            restore_request(1, RestoreFiles::Only(&[]), 4, 5),
            "NoFilesChosen",
        ),
        (
            restore_request(99, RestoreFiles::All, 4, 5),
            "UnknownCheckpoint",
        ),
        (
            restore_request(1, RestoreFiles::All, 3, 5),
            "DuplicateCheckpoint",
        ),
        (
            restore_request(1, RestoreFiles::All, 5, 5),
            "DuplicateCheckpoint",
        ),
    ];
    for (request, want) in cases {
        let err = matter.restore(&request).unwrap_err();
        let got = match &err {
            Error::UnknownFile { .. } => "UnknownFile",
            Error::NoFilesChosen => "NoFilesChosen",
            Error::UnknownCheckpoint { .. } => "UnknownCheckpoint",
            Error::DuplicateCheckpoint { .. } => "DuplicateCheckpoint",
            other => panic!("{other:?}"),
        };
        assert_eq!(got, want, "{err:?}");
        assert_eq!(err.code(), ErrorCode::CommandInvalid);
        assert_eq!(fx.read("a.txt"), b"an outside edit");
        assert!(!fx.exists("b.txt"));
        assert_eq!(matter.timeline().len(), 2);
    }
    assert!(matches!(
        matter.restore(&restore_request(1, RestoreFiles::Only(&unknown), 4, 5)),
        Err(Error::UnknownFile { path: p }) if p == path("nowhere.txt")
    ));
    assert_no_temp_files(&fx.folder);
}

#[test]
fn restore_leaves_cloud_only_and_over_cap_files() {
    let mut fx = Fixture::new("restore-skipped");
    fx.limits.file_cap_bytes = 10;
    fx.write("a.txt", "one");
    fx.write("big.txt", "small");
    fx.write("cloud.txt", "sea");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    fx.write("cloud.txt", "sea two");
    created(
        matter
            .after_turn(&meta(3), &tools(&["a.txt", "cloud.txt"]))
            .unwrap(),
        3,
    );
    // Now over the cap, and cloud-only; on Unix neither can be read at all.
    fx.write("big.txt", "far too large for the cap");
    fx.probe.mark(fx.file("cloud.txt"));
    #[cfg(unix)]
    for name in ["big.txt", "cloud.txt"] {
        support::set_mode(fx.file(name), 0o000);
    }

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_eq!(restored.written, paths(&["a.txt"]));
    assert_eq!(restored.kept, paths(&["big.txt", "cloud.txt"]));
    #[cfg(unix)]
    for name in ["big.txt", "cloud.txt"] {
        support::set_mode(fx.file(name), 0o644);
    }
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(fx.read("big.txt"), b"far too large for the cap");
    assert_eq!(fx.read("cloud.txt"), b"sea two");

    // Chosen by name, they are kept too.
    let chosen = paths(&["big.txt"]);
    let preview = matter
        .preview_restore(cid(1), RestoreFiles::Only(&chosen))
        .unwrap();
    assert_eq!(preview.kept, chosen);
    assert_none(&preview.writes);
}

#[test]
fn restore_refuses_a_target_written_after_its_outside_edits_checkpoint() {
    let fx = Fixture::new("restore-moved-since");
    // Asked about a.txt once by the restore's walk, then by its writable check.
    let probe = Arc::new(TouchingProbe {
        target: fx.file("a.txt"),
        nth: 2,
        armed: AtomicBool::new(false),
        seen: AtomicUsize::new(0),
    });
    let settings = Settings {
        probe: Arc::<TouchingProbe>::clone(&probe),
        ..fx.settings()
    };
    fx.write("a.txt", "one");
    let cancel = AtomicBool::new(false);
    let imported = Matter::import(settings, fx.new_matter(), &[], &mut |_| {}, &cancel);
    let Ok(Imported::Done { matter, .. }) = imported else {
        panic!("{imported:?}");
    };
    let mut matter = *matter;
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    created(matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap(), 3);

    probe.armed.store(true, Ordering::Relaxed);
    let err = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap_err();
    assert!(
        matches!(&err, Error::FileLocked { paths } if *paths == [path("a.txt")]),
        "{err:?}"
    );
    assert_eq!(probe.seen.load(Ordering::Relaxed), 2);
    assert_eq!(fx.read("a.txt"), b"two");
    assert_no_temp_files(&fx.folder);
}

#[test]
fn restore_recreates_deleted_subfolder() {
    let fx = Fixture::new("restore-subfolder");
    fx.write("sub/deep/c.txt", "deep");
    let mut matter = fx.import();
    support::remove_dir_all(fx.file("sub"));

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_eq!(
        created(restored.outside_edits.unwrap(), 4),
        [deleted("sub/deep/c.txt", b"deep")]
    );
    assert_eq!(restored.written, paths(&["sub/deep/c.txt"]));
    assert_eq!(fx.read("sub/deep/c.txt"), b"deep");
}

#[test]
fn restore_keeps_a_name_the_folder_spells_otherwise() {
    let fx = Fixture::new("restore-case");
    if !insensitive(&fx, "Memo.txt", "memo.txt", CASE_REQUIRED) {
        return;
    }
    fx.write("Memo.txt", "old");
    let mut matter = fx.import();
    support::remove_file(fx.file("Memo.txt"));
    fx.write("memo.txt", "the lawyer's own memo");

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_none(&restored.written);
    assert_eq!(restored.kept, paths(&["Memo.txt"]));
    assert_eq!(restored.added_since, paths(&["memo.txt"]));
    assert_eq!(names(&fx.folder), ["memo.txt"]);
    assert_eq!(fx.read("memo.txt"), b"the lawyer's own memo");
    assert_no_temp_files(&fx.folder);
}

#[test]
fn preview_matches_restore_and_writes_nothing() {
    let (fx, mut matter) = versioned("restore-preview");
    fx.write("d.txt", "an outside edit, not yet recorded");
    let objects = count_files(&fx.git_dir().join("objects"));
    let index = std::fs::read(fx.git_dir().join("index")).unwrap();

    let preview = matter.preview_restore(cid(1), RestoreFiles::All).unwrap();
    assert_eq!(count_files(&fx.git_dir().join("objects")), objects);
    assert_eq!(std::fs::read(fx.git_dir().join("index")).unwrap(), index);
    assert_eq!(matter.timeline().len(), 2);
    assert_none(&preview.locked);

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    let written: Vec<MatterPath> = preview.writes.iter().map(|w| w.path().clone()).collect();
    assert_eq!(restored.written, written);
    assert_eq!(restored.added_since, preview.added_since);
    assert_eq!(restored.kept, preview.kept);
    assert_eq!(created(restored.checkpoint, 5), preview.writes);
}
