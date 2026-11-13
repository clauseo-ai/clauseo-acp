//! Which places a turn's tool calls named count as written by the agent
//! ([`clauseo_matters::names_a_write`]), so the after-turn checkpoint leaves a change there
//! unflagged: a Read (or any look) at a file the lawyer saves during the turn never clears the
//! flag, so Undo warns and keeps it by default; an Edit's location, a diff's path and a shell
//! command's write do clear it.

use std::collections::BTreeSet;
use std::time::{Duration, SystemTime};

use clauseo_matters::{Matter, names_a_write};
use clauseo_protocol::{FileChange, MatterPath, ToolCallStatus, ToolKind};

use crate::fixture::{Fixture, at, created, meta};
use crate::support;

/// One tool call naming `note.txt`: its kind, its status, and whether the name is in its diff.
type Call = (ToolKind, ToolCallStatus, bool);

/// The paths of `calls` that count as written ([`names_a_write`]), all naming `note.txt`.
fn written(calls: &[Call]) -> BTreeSet<MatterPath> {
    calls
        .iter()
        .filter(|(kind, status, in_diff)| names_a_write(*kind, *status, *in_diff))
        .map(|_| MatterPath::new("note.txt").unwrap())
        .collect()
}

/// A matter holding `a.txt` (and `note.txt` when `before`), its turn 2 started; `note.txt` is
/// then saved during the turn, `calls` name it and the shell commands write `commands`, and the
/// turn ends with checkpoint 3: its changes.
fn turn(name: &str, before: bool, calls: &[Call], commands: &[&str]) -> Vec<FileChange> {
    let fx = Fixture::new(name);
    fx.write("a.txt", "one");
    if before {
        fx.write("note.txt", "the lawyer's note");
    }
    let mut matter: Matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("note.txt", "the lawyer's note, saved during the turn");
    // Saved a second before the turn's end, as a file the turn wrote would be.
    let millis = u64::try_from(at(3).as_millis() - 1000).unwrap();
    support::set_mtime(
        fx.file("note.txt"),
        SystemTime::UNIX_EPOCH + Duration::from_millis(millis),
    );
    let commands: BTreeSet<String> = commands.iter().map(|path| (*path).to_owned()).collect();
    let checkpoint = matter
        .after_turn_named(&meta(3), &written(calls), &commands)
        .unwrap();
    created(checkpoint, 3)
}

/// Whether the turn's one change, at `note.txt`, is flagged.
fn flagged(name: &str, before: bool, calls: &[Call], commands: &[&str]) -> bool {
    let changes = turn(name, before, calls, commands);
    assert_eq!(changes.len(), 1, "{changes:?}");
    assert_eq!(changes[0].path().as_str(), "note.txt");
    changes[0].may_include_lawyer_edits()
}

const DONE: ToolCallStatus = ToolCallStatus::Completed;

/// A file saved during the turn that the agent only read, searched or ran a command in, new or
/// there before, stays flagged: a look is no write.
#[test]
fn a_read_never_clears_the_flag() {
    for kind in [
        ToolKind::Read,
        ToolKind::Search,
        ToolKind::Fetch,
        ToolKind::Think,
        ToolKind::Execute,
        ToolKind::Other,
    ] {
        for before in [false, true] {
            let name = format!("written-look-{kind:?}-{before}").to_lowercase();
            assert!(
                flagged(&name, before, &[(kind, DONE, false)], &[]),
                "a {kind:?} location cleared the flag (there before: {before})"
            );
        }
    }
}

/// An Edit, Delete or Move call's location, or a diff's path of any call, is the agent's write.
#[test]
fn an_edit_or_a_diff_clears_the_flag() {
    let calls: [(&str, Call); 5] = [
        ("edit", (ToolKind::Edit, DONE, false)),
        ("delete", (ToolKind::Delete, DONE, false)),
        ("move", (ToolKind::Move, DONE, false)),
        ("diff-of-edit", (ToolKind::Edit, DONE, true)),
        ("diff-of-other", (ToolKind::Other, DONE, true)),
    ];
    for (name, call) in calls {
        for before in [false, true] {
            assert!(
                !flagged(&format!("written-{name}-{before}"), before, &[call], &[]),
                "{name} left the flag (there before: {before})"
            );
        }
    }
}

/// A Read alongside an Edit of the same file: the Edit counts.
#[test]
fn a_read_and_an_edit_of_one_file_clear_the_flag() {
    let calls = [(ToolKind::Read, DONE, false), (ToolKind::Edit, DONE, false)];
    assert!(!flagged("written-read-and-edit", true, &calls, &[]));
}

/// An Edit that ended `Failed` wrote nothing it named.
#[test]
fn a_failed_edit_does_not_clear_the_flag() {
    let call = (ToolKind::Edit, ToolCallStatus::Failed, true);
    assert!(flagged("written-failed-edit", true, &[call], &[]));
}

/// A file a shell command of the turn wrote is the agent's own (`after_turn_named`), even read
/// as well: a new one (`stetdoc edit`'s default `NAME.edited.docx`) and one there before, which
/// it modified (`stetdoc edit --in-place`). Named by no command, the old one stays flagged.
#[test]
fn a_shell_write_clears_the_flag_of_a_new_or_modified_file() {
    let read = [(ToolKind::Read, DONE, false)];
    assert!(!flagged("written-shell-new", false, &read, &["note.txt"]));
    assert!(!flagged("written-shell-old", true, &read, &["note.txt"]));
    assert!(flagged("written-shell-other", true, &read, &["other.txt"]));
}
