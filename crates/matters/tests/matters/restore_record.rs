//! A restore's steps before it writes, alone (`Matter::restore_record`): the folder is recorded
//! as the `outside-edits` checkpoint and no file is written; it refuses as the restore does,
//! before recording anything; and a restore that follows, with fresh ids, reuses the checkpoint.

use clauseo_matters::{CheckpointOutcome, Error, Matter, RestoreFiles};
use clauseo_protocol::{CheckpointId, CheckpointKind, ErrorCode, MatterPath};

use crate::fixture::{
    Fixture, assert_no_temp_files, cid, meta, path, read_at, restore_request, tools,
};
use crate::support;

/// Checkpoint 1 holds `a.txt` "one" and `b.txt` "bee"; checkpoint 3, after a turn, `a.txt`
/// "two".
fn versioned(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    fx.write("b.txt", "bee");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap();
    (fx, matter)
}

fn ids(matter: &Matter) -> Vec<CheckpointId> {
    matter.timeline().iter().map(|entry| entry.id).collect()
}

#[test]
fn the_record_writes_nothing_and_the_restore_after_it_reuses_its_checkpoint() {
    let (fx, mut matter) = versioned("restore-record-then-restore");
    fx.write("a.txt", "three, by hand");
    let chosen = [path("a.txt")];

    let outside = matter
        .restore_record(&restore_request(1, RestoreFiles::Only(&chosen), 4, 5))
        .unwrap();
    assert!(matches!(outside.outcome, CheckpointOutcome::Created(id) if id == cid(4)));
    assert_eq!(fx.read("a.txt"), b"three, by hand", "nothing is written");
    assert_no_temp_files(&fx.folder);
    assert_eq!(ids(&matter), [cid(1), cid(3), cid(4)]);
    assert_eq!(matter.timeline()[2].kind, CheckpointKind::OutsideEdits);
    assert_eq!(read_at(&matter, 4, "a.txt"), b"three, by hand");

    // Recorded again with nothing changed: the same checkpoint, reused.
    let again = matter
        .restore_record(&restore_request(1, RestoreFiles::Only(&chosen), 6, 7))
        .unwrap();
    assert!(matches!(again.outcome, CheckpointOutcome::Reused(id) if id == cid(4)));
    // Its id is taken: a restore that names it again is refused, so the restore after a
    // record has fresh ids.
    let err = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 4, 8))
        .unwrap_err();
    assert!(
        matches!(&err, Error::DuplicateCheckpoint { id } if *id == cid(4)),
        "{err:?}"
    );

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::Only(&chosen), 8, 9))
        .unwrap();
    assert!(
        restored.outside_edits.is_none(),
        "the record's checkpoint is reused"
    );
    assert_eq!(restored.written, chosen);
    assert!(matches!(restored.checkpoint.outcome, CheckpointOutcome::Created(id) if id == cid(9)));
    assert_eq!(fx.read("a.txt"), b"one");
    assert_eq!(ids(&matter), [cid(1), cid(3), cid(4), cid(9)]);
}

/// The record and the restore give the same refusal, and the record records nothing.
fn assert_both_refuse(matter: &mut Matter, files: RestoreFiles<'_>, code: ErrorCode) {
    let before = ids(matter);
    let recorded = matter
        .restore_record(&restore_request(1, files, 4, 5))
        .unwrap_err();
    assert_eq!(ids(matter), before, "nothing is recorded: {recorded:?}");
    let restored = matter
        .restore(&restore_request(1, files, 4, 5))
        .unwrap_err();
    assert_eq!(recorded.code(), code, "{recorded:?}");
    assert_eq!(recorded.to_string(), restored.to_string());
    assert_eq!(ids(matter), before);
}

#[test]
fn the_record_refuses_as_the_restore_does() {
    let (fx, mut matter) = versioned("restore-record-refusals");
    fx.write("b.txt", "an outside edit");

    // A file open in Word.
    fx.write("~$a.txt", "owner file: open in Word");
    assert_both_refuse(&mut matter, RestoreFiles::All, ErrorCode::MatterFileLocked);
    support::remove_file(fx.file("~$a.txt"));

    // A chosen file whose name is a folder now.
    support::remove_file(fx.file("a.txt"));
    fx.write("a.txt/inside.txt", "a folder where the file was");
    let chosen = [path("a.txt")];
    let clash = ErrorCode::MatterFileFolderClash;
    assert_both_refuse(&mut matter, RestoreFiles::Only(&chosen), clash);

    // A chosen file neither the version nor the folder has, and no file chosen.
    let unknown: [MatterPath; 1] = [path("never.txt")];
    let code = Error::UnknownFile {
        path: path("never.txt"),
    }
    .code();
    assert_both_refuse(&mut matter, RestoreFiles::Only(&unknown), code);
    let code = Error::NoFilesChosen.code();
    assert_both_refuse(&mut matter, RestoreFiles::Only(&[]), code);
    assert_eq!(fx.read("b.txt"), b"an outside edit");
    assert_no_temp_files(&fx.folder);
}
