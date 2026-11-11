use clauseo_protocol::{MatterPath, Timestamp};

use super::{NoteInput, RestoreNote, Told, add_undone, note_text, since_last_turn};

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
