//! A tool call counts as the agent's write only once it ends `Completed`
//! ([`clauseo_matters::names_a_write`], the host's `TurnPaths::shell_writes`): a denied shell
//! command, an edit whose ask was open when Stop was pressed, and an edit still pending at the
//! turn's end (settled `Completed` by the host's close only after the checkpoint, #1038) wrote
//! nothing. A file of that name the lawyer saved meanwhile stays flagged, so Undo asks and keeps
//! it by default instead of deleting or rolling back the lawyer's work.

use std::path::Path;

use crate::lawyer_file::{Answer, Ran, answered_turn, edited_since, flag};
use crate::support::START_MS;
use crate::write;

/// Dates `file` a minute before the turn's end (the test clock runs behind the real one), as a
/// file the agent made would be.
fn before_end(file: &Path) {
    let at = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(u64::try_from(START_MS - 60_000).unwrap());
    write::set_mtime(file, at);
}

fn assert_flagged(ran: Ran, path: &str) {
    let (_dir, host, thread, turn, _folder, flags) = ran;
    assert_eq!(flags, [flag(path, true)]);
    assert_eq!(edited_since(&host, thread, turn), Vec::<String>::new());
    host.shutdown();
}

/// `pandoc in.md -o brief.docx`, denied, ended failed; the lawyer's own brief.docx, saved
/// while the ask waited, stays flagged.
#[test]
fn a_denied_shell_commands_target_saved_by_the_lawyer_stays_flagged() {
    let ran = answered_turn(
        "lawyer_status_denied_shell",
        "undo-lawyer-file-denied-shell",
        &[("in.md", "# Brief\n")],
        |folder| {
            let brief = folder.join("brief.docx");
            write::write(&brief, b"the lawyer's brief\n");
            before_end(&brief);
        },
        Answer::Reject,
    );
    assert_flagged(ran, "brief.docx");
}

/// An edit of memo.docx whose ask was open when Stop was pressed; the lawyer's edit of memo.docx
/// meanwhile stays flagged.
#[test]
fn an_edit_stopped_at_its_ask_leaves_the_lawyers_edit_flagged() {
    let ran = answered_turn(
        "lawyer_status_stop_edit",
        "undo-lawyer-file-stop-edit",
        &[("memo.docx", "memo\n")],
        |folder| write::write(&folder.join("memo.docx"), b"the lawyer's memo\n"),
        Answer::Stop,
    );
    assert_flagged(ran, "memo.docx");
}

/// #1038's sweep: an edit still pending at `end_turn` is settled `Completed` by the host's close,
/// after the checkpoint; the lawyer's edit of the file it names stays flagged.
#[test]
fn an_edit_pending_at_the_turns_end_leaves_the_lawyers_edit_flagged() {
    let ran = answered_turn(
        "lawyer_status_pending_sweep",
        "undo-lawyer-file-pending-sweep",
        &[("memo.docx", "memo\n"), ("brief.txt", "Clause 4 draft\n")],
        |folder| write::write(&folder.join("memo.docx"), b"the lawyer's memo\n"),
        Answer::Allow,
    );
    assert_flagged(ran, "memo.docx");
}
