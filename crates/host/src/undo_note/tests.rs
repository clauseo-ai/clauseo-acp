use clauseo_protocol::{MatterPath, Timestamp};

use clauseo_protocol::CheckpointId;

use super::{
    NoteInput, RESTORE_FILES_CAP, RestoreNote, Told, add_undone, capped, note_text,
    since_last_turn, with_restores,
};

fn paths(names: &[&str]) -> Vec<MatterPath> {
    names.iter().map(|n| n.parse().unwrap()).collect()
}

/// 2026-11-11 at 14:05 UTC, plus `secs`.
fn at(secs: i64) -> Timestamp {
    Timestamp::new(1_794_405_900_000 + secs * 1000).unwrap()
}

fn restore(back: &[&str], removed: &[&str], secs: i64) -> RestoreNote {
    RestoreNote {
        back: paths(back),
        removed: paths(removed),
        more: 0,
        checkpoint: None,
        at: at(secs),
    }
}

#[test]
fn lists_undos_since_last_turn_once_each() {
    let before = paths(&["old.txt"]);
    let undone = paths(&["notes/a.txt", "brief.txt"]);
    let again = paths(&["brief.txt"]);
    let inputs = [
        NoteInput::Undone {
            restored: &before,
            at: at(0),
        },
        NoteInput::TurnStarted,
        NoteInput::Undone {
            restored: &undone,
            at: at(1),
        },
        NoteInput::Undone {
            restored: &again,
            at: at(2),
        },
    ];
    let told = since_last_turn(inputs, Vec::new());
    assert_eq!(
        note_text(&told).unwrap(),
        "Note from Clauseo: since your last turn, the lawyer put these files back to an earlier \
         version, and removed those it did not have: notes/a.txt, brief.txt. Read them again \
         before relying on what you remember about them."
    );

    let after_turn = since_last_turn(
        [
            NoteInput::Undone {
                restored: &undone,
                at: at(1),
            },
            NoteInput::TurnStarted,
        ],
        Vec::new(),
    );
    assert_eq!(after_turn, []);
    assert_eq!(note_text(&after_turn), None);
}

/// Stage C: Undo removed the reply, then a Versions restore brought it back. Two lines, in
/// order, so the agent knows it is back.
#[test]
fn a_restore_after_an_undo_has_its_own_line_after_it() {
    let undone = paths(&["reply.docx"]);
    let told = since_last_turn(
        [NoteInput::Undone {
            restored: &undone,
            at: at(0),
        }],
        vec![restore(&["reply.docx"], &[], 60)],
    );
    assert_eq!(
        note_text(&told).unwrap(),
        "Note from Clauseo: since your last turn, the lawyer put these files back to an earlier \
         version, and removed those it did not have: reply.docx.\nThe lawyer restored reply.docx \
         from Versions; it's back as of 2026-11-11 14:06 UTC. Read them again before relying on \
         what you remember about them."
    );
}

/// A restore held in memory goes before a later undo the log has; an undo after it is told
/// again, after it.
#[test]
fn restores_and_undos_keep_their_order() {
    let undone = paths(&["a.txt"]);
    let mut told = since_last_turn(
        [NoteInput::Undone {
            restored: &undone,
            at: at(10),
        }],
        vec![restore(&["a.txt", "b.txt"], &["c.txt"], 5)],
    );
    assert_eq!(
        told,
        [
            Told::Restored(restore(&["a.txt", "b.txt"], &["c.txt"], 5)),
            Told::Undone(paths(&["a.txt"])[0].clone()),
        ]
    );
    // The same undo, from the host's memory too, is told once.
    add_undone(&mut told, &paths(&["a.txt"])[0]);
    assert_eq!(told.len(), 2);
    assert_eq!(
        note_text(&told).unwrap(),
        "Note from Clauseo: The lawyer restored a.txt, b.txt from Versions; they're back as of \
         2026-11-11 14:05 UTC.\nThat restore also removed c.txt, which that version did not \
         have.\nThen the lawyer put these files back to an earlier version, and removed those it \
         did not have: a.txt. Read them again before relying on what you remember about them."
    );
}

#[test]
fn a_restore_that_only_removed_says_so() {
    let told = [Told::Restored(restore(&[], &["draft.docx"], 0))];
    assert_eq!(
        note_text(&told).unwrap(),
        "Note from Clauseo: The lawyer restored an earlier version from Versions as of \
         2026-11-11 14:05 UTC; it removed draft.docx, which that version did not have. Read them \
         again before relying on what you remember about them."
    );
}

/// A checkpoint id, distinct per `n`.
fn checkpoint(n: u8) -> CheckpointId {
    let id = uuid::Builder::from_unix_timestamp_millis(1_790_000_000_000, &[n; 10]).into_uuid();
    CheckpointId::new(id).unwrap()
}

/// A restore the log holds and the host's memory holds too (the note was first read after it)
/// is told once, from the log; one only memory holds (its commit failed) is told as well.
#[test]
fn a_restore_in_the_log_and_in_memory_is_told_once() {
    let logged = RestoreNote {
        checkpoint: Some(checkpoint(1)),
        ..restore(&["a.txt"], &[], 0)
    };
    let held = RestoreNote {
        at: at(1),
        ..logged.clone()
    };
    let failed = RestoreNote {
        checkpoint: Some(checkpoint(2)),
        ..restore(&["b.txt"], &[], 5)
    };
    let told = since_last_turn([], vec![logged.clone(), held, failed.clone()]);
    assert_eq!(told, [Told::Restored(logged), Told::Restored(failed)]);
}

/// Every restore given is after the last turn that carried the note, so one with an earlier
/// time (a clock that stepped back) is still told, among the undos after that turn only.
#[test]
fn a_restore_is_put_only_among_the_undos_after_the_last_turn() {
    let (before, after) = (paths(&["a.txt"]), paths(&["b.txt"]));
    let inputs = [
        NoteInput::Undone {
            restored: &before,
            at: at(10),
        },
        NoteInput::TurnStarted,
        NoteInput::Undone {
            restored: &after,
            at: at(20),
        },
    ];
    let early = restore(&["c.txt"], &[], 5);
    let told = since_last_turn(inputs, vec![early.clone()]);
    assert_eq!(
        told,
        [Told::Restored(early), Told::Undone(after[0].clone())]
    );
}

/// The note read in full later keeps the partial one's trims and its restores only memory held.
#[test]
fn reading_the_note_in_full_keeps_what_memory_changed() {
    let (a, b) = (paths(&["a.txt"])[0].clone(), paths(&["b.txt"])[0].clone());
    let logged = RestoreNote {
        checkpoint: Some(checkpoint(1)),
        ..restore(&["c.txt"], &[], 0)
    };
    let failed = RestoreNote {
        checkpoint: Some(checkpoint(2)),
        ..restore(&["d.txt"], &[], 9)
    };
    // b.txt came back and was taken out of the partial note.
    let partial = vec![Told::Undone(a.clone()), Told::Restored(failed.clone())];
    let full = vec![
        Told::Restored(logged.clone()),
        Told::Undone(a.clone()),
        Told::Undone(b),
    ];
    assert_eq!(
        with_restores(partial, full),
        [
            Told::Restored(logged),
            Told::Undone(a),
            Told::Restored(failed)
        ]
    );
}

/// Each list is cut at the cap, and the note counts what was cut.
#[test]
fn a_large_restore_names_the_first_files_and_counts_the_rest() {
    let many = |prefix: &str| -> Vec<MatterPath> {
        (0..RESTORE_FILES_CAP + 2)
            .map(|n| format!("{prefix}{n}.txt").parse().unwrap())
            .collect()
    };
    let (written, removed, more) = capped(many("w"), many("r"));
    assert_eq!(
        (written.len(), removed.len(), more),
        (RESTORE_FILES_CAP, RESTORE_FILES_CAP, 4)
    );
    let note = RestoreNote {
        more,
        ..restore(&["a.txt"], &[], 0)
    };
    let text = note_text(&[Told::Restored(note)]).unwrap();
    assert!(
        text.contains("\nThat restore also changed 4 more files not named here. Read them"),
        "{text}"
    );
}
