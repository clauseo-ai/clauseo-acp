//! `PreviewUndo` (#360): which of a step's files changed after its after-turn checkpoint, by
//! content. A file the agent wrote with a shell command (no tool call names it, so its
//! `FileChange` is flagged `may_include_lawyer_edits`) and left alone is not edited since; a
//! later edit is, whoever made it.

use clauseo_protocol::{
    ErrorCode, FileChoice, FileDecision, HostCommand, HostQuery, MatterPath, QueryResult,
    StopReason, StreamId, ThreadId, TurnId,
};

use crate::support::{Dir, FAKE, Running, Scenario, foreign_uuid, matter_with_thread};
use crate::write;

/// A host whose fake agent ran `undo-preview`'s one turn: `notes.txt` written by a shell
/// command, `memo.txt` by an edit tool call. Returns the thread, the turn and the folder.
fn after_one_turn(test: &str) -> (Dir, Running, ThreadId, TurnId, std::path::PathBuf) {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local("undo-preview"))]);
    let host = dir.start(0);
    let (_, thread, folder) =
        matter_with_thread(&dir, &host, "m", &[("brief.txt", "Clause 4 draft\n")], FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(thread, "Write the notes.");
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, StopReason::EndTurn, "{}", events.dump());
    assert!(ended.checkpoint_after.is_some(), "{}", events.dump());
    let changed = events
        .files_changed(turn)
        .unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()));
    let flags: Vec<(&str, bool)> = changed
        .files
        .iter()
        .map(|file| (file.path().as_str(), file.may_include_lawyer_edits()))
        .collect();
    // The flag the thread's Undo read before #360: set on the shell-written file although
    // nothing touched it after the turn.
    assert_eq!(flags, [("memo.txt", false), ("notes.txt", true)]);
    (dir, host, thread, turn, folder)
}

fn edited_since(host: &Running, thread: ThreadId, turn: TurnId) -> Vec<String> {
    let answer = host
        .query(HostQuery::PreviewUndo { thread, turn })
        .unwrap_or_else(|error| panic!("PreviewUndo refused: {error}"));
    let QueryResult::UndoPreview(preview) = answer else {
        panic!("PreviewUndo answered {answer:?}");
    };
    preview
        .edited_since
        .iter()
        .map(|path| path.as_str().to_owned())
        .collect()
}

/// The repro of #360: nothing touched either file after the turn, so nothing is edited since,
/// the shell-written file included; and the step still undoes.
#[test]
fn a_shell_write_left_alone_is_not_edited_since() {
    let (_dir, host, thread, turn, folder) = after_one_turn("preview_undo_shell_write_left_alone");
    assert_eq!(std::fs::read(folder.join("notes.txt")).unwrap(), b"hello\n");
    assert_eq!(edited_since(&host, thread, turn), Vec::<String>::new());
    // Asking again (now the cache may vouch by stat) gives the same.
    assert_eq!(edited_since(&host, thread, turn), Vec::<String>::new());
    host.shutdown();
}

/// A later edit of the shell-written file, the same size and right after the checkpoint (where
/// a stat alone could miss it), is listed; the tool-call file left alone is not. Deleting that
/// one lists it too.
#[test]
fn an_edit_after_the_checkpoint_is_edited_since() {
    let (_dir, host, thread, turn, folder) = after_one_turn("preview_undo_edit_after");
    write::write(&folder.join("notes.txt"), b"HELLO\n");
    assert_eq!(edited_since(&host, thread, turn), ["notes.txt"]);
    write::remove_file(&folder.join("memo.txt"));
    assert_eq!(edited_since(&host, thread, turn), ["memo.txt", "notes.txt"]);
    // Put back as it was, it is no longer edited since.
    write::write(&folder.join("notes.txt"), b"hello\n");
    assert_eq!(edited_since(&host, thread, turn), ["memo.txt"]);
    host.shutdown();
}

/// A turn with no step to undo is refused as `UndoStep` refuses it: a turn this thread never
/// ran, and a step undone already; an unknown thread is `command.unknown_thread`.
#[test]
fn a_turn_with_no_step_is_refused() {
    let (_dir, host, thread, turn, _folder) = after_one_turn("preview_undo_no_step");
    let stranger = TurnId::new(foreign_uuid(7)).unwrap();
    let refusal = host
        .query(HostQuery::PreviewUndo {
            thread,
            turn: stranger,
        })
        .unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal}");
    let nobody = ThreadId::new(foreign_uuid(8)).unwrap();
    let refusal = host
        .query(HostQuery::PreviewUndo {
            thread: nobody,
            turn,
        })
        .unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandUnknownThread, "{refusal}");

    host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: ["memo.txt", "notes.txt"]
            .map(|path| FileDecision {
                path: MatterPath::new(path).unwrap(),
                choice: FileChoice::Undo,
            })
            .into(),
    });
    host.settle();
    let refusal = host
        .query(HostQuery::PreviewUndo { thread, turn })
        .unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal}");
    assert!(refusal.to_string().contains("no step to undo"), "{refusal}");
    host.shutdown();
}
