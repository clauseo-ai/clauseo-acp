use clauseo_protocol::MatterPath;

use super::{NoteInput, note_text, paths_since_last_turn};

fn paths(names: &[&str]) -> Vec<MatterPath> {
    names.iter().map(|n| n.parse().unwrap()).collect()
}

#[test]
fn lists_undo_and_restore_since_last_turn() {
    let before = paths(&["old.txt"]);
    let undone = paths(&["notes/a.txt", "brief.txt"]);
    let restored = paths(&["brief.txt", "notes/a.txt"]);
    let inputs = [
        NoteInput::Undone { restored: &before },
        NoteInput::TurnStarted,
        NoteInput::Undone { restored: &undone },
        NoteInput::Restored { paths: &restored },
    ];
    let listed = paths_since_last_turn(inputs);
    assert_eq!(listed, paths(&["notes/a.txt", "brief.txt"]));
    assert_eq!(
        note_text(&listed).unwrap(),
        "Note from Clauseo: since your last turn, the lawyer put these files back to an earlier \
         version: notes/a.txt, brief.txt. Read them again before relying on what you remember \
         about them."
    );

    let after_turn = paths_since_last_turn([
        NoteInput::Undone { restored: &undone },
        NoteInput::TurnStarted,
    ]);
    assert_eq!(after_turn, Vec::<MatterPath>::new());
    assert_eq!(note_text(&after_turn), None);
}
