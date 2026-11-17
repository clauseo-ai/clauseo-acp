//! An undo of a restore that removes a file the restore brought back: into the trash, or, when
//! the trash can't take it, deleted for good (Versions holds it). The logged `VersionRestored`
//! says which in `deleted`, so the app's line never says the trash for a file deleted for good.
//! And an undo of a restore the log has no record of is refused.

use clauseo_protocol::{
    CheckpointKind, CommandOutcome, DomainEvent, ErrorCode, HostCommand, HostQuery, MatterPath,
    RestoreRequest, VersionRestored,
};

use crate::support::{Dir, FAKE, Scenario, matter_with_thread};
use crate::versions::{matter_events, recorded};
use crate::write;

/// The matter's logged restores, in order.
fn restores(events: &crate::observe::Events) -> Vec<VersionRestored> {
    events
        .events()
        .filter_map(|event| {
            if let DomainEvent::VersionRestored(restored) = event {
                Some((**restored).clone())
            } else {
                None
            }
        })
        .collect()
}

#[test]
fn a_trash_failure_then_a_delete_for_good_is_reported_as_deleted_not_trashed() {
    for blocked in [false, true] {
        let name = if blocked {
            "undo_restore_deleted_for_good"
        } else {
            "undo_restore_trashed"
        };
        let dir = Dir::new(name, &[(FAKE, Scenario::Local("restore-note"))]);
        let host = dir.start(0);
        let (matter, _thread, folder) =
            matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
        let import = matter_events(&host, matter)
            .checkpoint(CheckpointKind::Import)
            .unwrap();
        let notes = folder.join("Reply notes.md");
        let restore = |checkpoint, undoes| HostCommand::RestoreVersion {
            matter,
            checkpoint,
            scope: RestoreRequest::Matter,
            undoes,
            thread: None,
        };
        // A version holding the notes, then the notes deleted and that version restored.
        write::write(&notes, b"notes\n");
        host.ok(restore(import, None));
        let with_notes = matter_events(&host, matter)
            .checkpoint(CheckpointKind::OutsideEdits)
            .unwrap();
        write::remove_file(&notes);
        let CommandOutcome::VersionRestored {
            checkpoint: brought,
        } = host.ok(restore(with_notes, None))
        else {
            panic!("the restore wrote nothing");
        };
        let before = recorded(&matter_events(&host, matter))
            .into_iter()
            .rev()
            .find(|(id, kind, _)| *kind == CheckpointKind::OutsideEdits && *id != with_notes)
            .map(|(id, _, _)| id)
            .unwrap();
        if blocked {
            // the trash's folder is a file: the trash can't take the notes
            write::write(&dir.trash(), b"a file, not a trash folder");
        }

        host.ok(restore(before, Some(brought)));
        assert!(!notes.exists(), "undoing the restore removed the notes");
        let logged = restores(&matter_events(&host, matter));
        let undo = logged.last().expect("the undo is logged");
        let name = MatterPath::new("Reply notes.md").unwrap();
        assert_eq!(undo.undoes, Some(brought));
        assert_eq!(undo.removed, std::slice::from_ref(&name));
        let for_good: &[MatterPath] = if blocked {
            std::slice::from_ref(&name)
        } else {
            &[]
        };
        assert_eq!(undo.deleted, for_good, "blocked: {blocked}");
        host.shutdown();
    }
}

/// An undo names the restore it takes back; one the matter's log has no `VersionRestored` for
/// (here the import, a version but no restore) is refused, by the preview and the restore
/// alike, and nothing is written: the host would not know what that restore put back.
#[test]
fn an_undo_of_a_restore_with_no_record_is_refused() {
    let dir = Dir::new(
        "undo_of_a_restore_with_no_record",
        &[(FAKE, Scenario::Local("restore-note"))],
    );
    let host = dir.start(0);
    let (matter, _thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let import = matter_events(&host, matter)
        .checkpoint(CheckpointKind::Import)
        .unwrap();
    let brief = folder.join("brief.txt");
    write::write(&brief, b"Clause 4 redrafted\n");

    let previewed = host
        .query(HostQuery::PreviewRestore {
            matter,
            checkpoint: import,
            scope: RestoreRequest::Matter,
            undoes: Some(import),
        })
        .unwrap_err();
    let restored = host
        .dispatch(HostCommand::RestoreVersion {
            matter,
            checkpoint: import,
            scope: RestoreRequest::Matter,
            undoes: Some(import),
            thread: None,
        })
        .unwrap_err();
    for error in [previewed, restored] {
        assert_eq!(error.code(), ErrorCode::CommandInvalid, "{error:?}");
        assert!(error.message().contains("no restore"), "{error:?}");
    }
    assert_eq!(
        std::fs::read_to_string(&brief).unwrap(),
        "Clause 4 redrafted\n"
    );
    host.shutdown();
}
