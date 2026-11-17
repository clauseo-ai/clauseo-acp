//! A restore logs the copy it writes over before it writes: the folder as it was is in Versions
//! (its `CheckpointRecorded` in the matter's stream, durable) before any file is written, so a
//! restore that fails or is killed while writing never leaves that copy in the history alone.
//! And a restore retried after one that was killed records the copy that one left unlogged.

use std::path::Path;
use std::sync::Arc;

use clauseo_matters::{
    FolderTrash, Limits, Matter, OsProbe, Restore, RestoreFiles, Retention, Settings,
};
use clauseo_protocol::{
    CheckpointId, CheckpointKind, CommandOutcome, DomainEvent, HostCommand, MatterId, MatterPath,
    RestoreRequest, Timestamp,
};

use crate::support::{Dir, FAKE, START_MS, Scenario, foreign_uuid, matter_with_thread};
use crate::versions::{matter_events, recorded};
use crate::write;

/// The restore of `brief.txt` alone, as checkpoint `from` has it.
fn restore_brief(matter: MatterId, from: CheckpointId) -> HostCommand {
    HostCommand::RestoreVersion {
        thread: None,
        undoes: None,
        matter,
        checkpoint: from,
        scope: RestoreRequest::Files {
            paths: vec![MatterPath::new("brief.txt").unwrap()],
        },
    }
}

/// Arms R8's failure before it writes `file`, by each spelling of its path: the matter may
/// hold its folder by another (`/private/var`). One spelling armed twice would fail the retry.
fn fail_before_writing(file: &Path) {
    clauseo_matters::test_support::fail_restore_write(file);
    let canonical = dunce::canonicalize(file).unwrap();
    if canonical != file {
        clauseo_matters::test_support::fail_restore_write(canonical);
    }
}

/// How many times the matter's log records checkpoint `id`.
fn times_recorded(host: &crate::support::Running, matter: MatterId, id: CheckpointId) -> usize {
    recorded(&matter_events(host, matter))
        .iter()
        .filter(|(one, _, _)| *one == id)
        .count()
}

/// R8 fails after it recorded the folder and before it writes (where a kill during the write
/// would stop it): the restore is refused, the hand edit stays, and the copy it would have
/// replaced is a version in the matter's log all the same. The retry restores, and the copy is
/// recorded once.
#[test]
fn a_restore_that_fails_while_writing_leaves_the_copy_it_replaced_in_versions() {
    let dir = Dir::new(
        "restore_fails_while_writing",
        &[(FAKE, Scenario::Local("restore-note"))],
    );
    let host = dir.start(0);
    let (matter, _thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    let brief = folder.join("brief.txt");
    write::write(&brief, b"Clause 4, edited by hand\n");
    fail_before_writing(&brief);

    let refused = host.dispatch(restore_brief(matter, import)).unwrap_err();
    let read = || std::fs::read_to_string(&brief).unwrap();
    assert_eq!(read(), "Clause 4, edited by hand\n");
    host.settle();
    let events = matter_events(&host, matter);
    let edits = events
        .checkpoint(CheckpointKind::OutsideEdits)
        .expect("the hand edit is a version though the restore failed");
    assert_eq!(events.checkpoint(CheckpointKind::Restore), None);
    assert!(
        !events
            .events()
            .any(|event| matches!(event, DomainEvent::VersionRestored(_))),
        "nothing was restored: {refused:?}"
    );

    let CommandOutcome::VersionRestored { checkpoint } = host.ok(restore_brief(matter, import))
    else {
        panic!("the retry answered otherwise");
    };
    assert_eq!(read(), "Clause 4 draft\n");
    let events = matter_events(&host, matter);
    assert_eq!(events.checkpoint(CheckpointKind::Restore), Some(checkpoint));
    assert_eq!(times_recorded(&host, matter, edits), 1, "{}", events.dump());
    host.shutdown();
}

/// R8's settings for the host of `dir`, as `matter_worker::matter_settings` makes them.
fn settings(dir: &Dir) -> Settings {
    Settings {
        matters_root: dir.data().join("matters"),
        refused_trees: vec![dir.data().to_path_buf()],
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe: Arc::new(OsProbe),
        trash: Arc::new(FolderTrash::new(dir.trash())),
    }
}

/// A restore killed while writing, when the host logged its outside-edits checkpoint only after
/// the write, left that checkpoint in the history and not in the matter's log (made here by R8
/// with the host stopped). The
/// retry's first step finds the folder equal to that checkpoint, so R8 reuses it, and the host
/// records it, with its row, before the restore writes.
#[test]
fn a_retried_restore_records_the_copy_a_killed_restore_left_unlogged() {
    let dir = Dir::new(
        "restore_records_an_orphan",
        &[(FAKE, Scenario::Local("restore-note"))],
    );
    let host = dir.start(0);
    let (matter, _thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    let brief = folder.join("brief.txt");
    write::write(&brief, b"Clause 4, edited by hand\n");
    host.shutdown();

    let orphan: CheckpointId = foreign_uuid(1).to_string().parse().unwrap();
    let restore: CheckpointId = foreign_uuid(2).to_string().parse().unwrap();
    let chosen = [MatterPath::new("brief.txt").unwrap()];
    let mut r8 = Matter::open(settings(&dir), matter, &folder).unwrap();
    fail_before_writing(&brief);
    let killed = r8
        .restore(&Restore {
            from: import,
            files: RestoreFiles::Only(&chosen),
            outside_edits: orphan,
            checkpoint: restore,
            at: Timestamp::new(START_MS + 1_800_000).unwrap(),
            undoes: None,
        })
        .unwrap_err();
    assert_eq!(
        r8.last_checkpoint(),
        Some(orphan),
        "the history holds it: {killed:?}"
    );
    drop(r8);

    let host = dir.start(1);
    assert_eq!(times_recorded(&host, matter, orphan), 0, "the log does not");
    let CommandOutcome::VersionRestored { checkpoint } = host.ok(restore_brief(matter, import))
    else {
        panic!("the retry answered otherwise");
    };
    assert_eq!(std::fs::read_to_string(&brief).unwrap(), "Clause 4 draft\n");
    let logged = recorded(&matter_events(&host, matter));
    let at = |id| logged.iter().position(|(one, _, _)| *one == id);
    let (Some(edits), Some(restored)) = (at(orphan), at(checkpoint)) else {
        panic!("not both recorded: {logged:?}");
    };
    assert_eq!(logged[edits].1, CheckpointKind::OutsideEdits);
    assert!(edits < restored, "{logged:?}");
    assert_eq!(times_recorded(&host, matter, orphan), 1);
    host.shutdown();
}
