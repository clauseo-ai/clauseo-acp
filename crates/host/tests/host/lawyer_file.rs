//! A file the lawyer saves into the matter while a turn runs, which the agent only reads (the
//! release dry run's `Lawyer note.txt`: "Read every paper…", a note saved mid-turn by the
//! lawyer's own scheduled task, then deleted by an Undo that never warned). A Read's location is
//! no write ([`clauseo_matters::names_a_write`]), so the step's `FileChange` stays flagged
//! `may_include_lawyer_edits`: the Undo dialogs warn ("may hold your own edits") and keep it by
//! default (`ui/app/src/room/versions/state.ts`, `defaultChoices`), and an Undo that keeps it
//! leaves it in the folder while the agent's own file goes back.

use std::path::{Path, PathBuf};

use clauseo_protocol::{
    AgentChoiceId, FileChoice, FileDecision, HostCommand, HostQuery, MatterPath, QueryResult,
    StopReason, StreamId, ThreadId, TurnId,
};

use crate::support::{Dir, FAKE, Running, START_MS, Scenario, matter_with_thread};
use crate::write;

const NOTE: &str = "Lawyer note.txt";

/// What a test's turn ran: the host, the thread, the turn, the folder and each changed file's
/// path and flag.
pub(crate) type Ran = (Dir, Running, ThreadId, TurnId, PathBuf, Vec<(String, bool)>);

/// How the test answers the turn's one permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Answer {
    /// Allow once; the turn ends `EndTurn`.
    Allow,
    /// Reject; the turn ends `EndTurn`.
    Reject,
    /// Stop the turn with the ask open; it ends `Cancelled`.
    Stop,
}

/// A host whose fake agent ran `undo-lawyer-file-read`'s turn, `save` run on the folder while
/// the agent's read waits for its permission.
fn turn_with(test: &str, save: impl FnOnce(&Path)) -> Ran {
    let files = [("brief.txt", "Clause 4 draft\n")];
    run_turn(test, "undo-lawyer-file-read", &files, save)
}

/// A host whose fake agent ran `scenario`'s turn in a matter holding `files`, `save` run on the
/// folder while the agent's one permission request waits, then allowed.
fn run_turn(
    test: &str,
    scenario: &'static str,
    files: &[(&str, &str)],
    save: impl FnOnce(&Path),
) -> Ran {
    answered_turn(test, scenario, files, save, Answer::Allow)
}

/// [`run_turn`], the permission request answered by `answer`.
pub(crate) fn answered_turn(
    test: &str,
    scenario: &'static str,
    files: &[(&str, &str)],
    save: impl FnOnce(&Path),
    answer: Answer,
) -> Ran {
    let dir = Dir::new(test, &[(FAKE, Scenario::Local(scenario))]);
    let host = dir.start(0);
    let (_, thread, folder) = matter_with_thread(&dir, &host, "m", files, FAKE);
    let mut events = host.events(StreamId::Thread(thread));
    let (turn, _) = host.prompt(
        thread,
        "Read every paper and write an overview in the chat.",
    );
    events.until("the read's permission request", |e| {
        !e.permission_requests().is_empty()
    });
    // The lawyer's save lands while the turn runs, between its two checkpoints.
    save(&folder);
    let (option, stop) = match answer {
        Answer::Allow => (Some("allow-once"), StopReason::EndTurn),
        Answer::Reject => (Some("reject-once"), StopReason::EndTurn),
        Answer::Stop => (None, StopReason::Cancelled),
    };
    match option {
        Some(option) => host.ok(HostCommand::AnswerPermission {
            thread,
            request: events.permission_requests()[0],
            option_id: AgentChoiceId::new(option).unwrap(),
        }),
        None => host.ok(HostCommand::Cancel { thread }),
    };
    let ended = events.until_turn_ended(turn);
    assert_eq!(ended.stop, stop, "{}", events.dump());
    let changed = events
        .files_changed(turn)
        .unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()));
    let flags = changed
        .files
        .iter()
        .map(|file| {
            (
                file.path().as_str().to_owned(),
                file.may_include_lawyer_edits(),
            )
        })
        .collect();
    (dir, host, thread, turn, folder, flags)
}

/// Dates `file` a minute before the turn's end (the test clock runs behind the real one), as
/// the agent's own new file is.
fn before_end(file: &Path) {
    let at = std::time::UNIX_EPOCH
        + std::time::Duration::from_millis(u64::try_from(START_MS - 60_000).unwrap());
    write::set_mtime(file, at);
}

pub(crate) fn flag(path: &str, flagged: bool) -> (String, bool) {
    (path.to_owned(), flagged)
}

pub(crate) fn edited_since(host: &Running, thread: ThreadId, turn: TurnId) -> Vec<String> {
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

/// The choices the Undo dialogs send by default: a flagged file kept, the rest undone.
fn default_choices(flags: &[(String, bool)]) -> Vec<FileDecision> {
    flags
        .iter()
        .map(|(path, flagged)| FileDecision {
            path: MatterPath::new(path).unwrap(),
            choice: if *flagged {
                FileChoice::Keep
            } else {
                FileChoice::Undo
            },
        })
        .collect()
}

/// The dry run's repro: a new note saved mid-turn that the agent reads stays flagged, so the
/// dialog warns and keeps it; the default Undo takes back only the agent's file.
#[test]
fn a_new_file_the_agent_only_read_is_flagged_and_kept() {
    let (_dir, host, thread, turn, folder, flags) = turn_with("lawyer_file_new_read", |folder| {
        write::write(&folder.join(NOTE), b"Call the client about clause 4.\n");
    });
    assert_eq!(flags, [flag(NOTE, true), flag("agent.txt", false)]);
    // Nothing changed after the turn: the preview's "edited since" is empty, so the flag alone
    // is what warns.
    assert_eq!(edited_since(&host, thread, turn), Vec::<String>::new());
    host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: default_choices(&flags),
    });
    assert_eq!(
        std::fs::read(folder.join(NOTE)).unwrap(),
        b"Call the client about clause 4.\n"
    );
    assert!(
        !folder.join("agent.txt").exists(),
        "the undo left agent.txt"
    );
    host.shutdown();
}

/// A paper there before the turn, which the lawyer edits mid-turn and the agent reads: flagged,
/// and the default Undo leaves the lawyer's edit in place.
#[test]
fn an_old_file_the_lawyer_edited_while_the_agent_read_it_is_flagged_and_kept() {
    let (_dir, host, thread, turn, folder, flags) = turn_with("lawyer_file_old_read", |folder| {
        write::write(
            &folder.join("brief.txt"),
            b"Clause 4 final, by the lawyer\n",
        );
    });
    assert_eq!(flags, [flag("agent.txt", false), flag("brief.txt", true)]);
    assert_eq!(edited_since(&host, thread, turn), Vec::<String>::new());
    host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: default_choices(&flags),
    });
    assert_eq!(
        std::fs::read(folder.join("brief.txt")).unwrap(),
        b"Clause 4 final, by the lawyer\n"
    );
    assert!(
        !folder.join("agent.txt").exists(),
        "the undo left agent.txt"
    );
    host.shutdown();
}

/// The dry run's own call sequence (its Claude transcript): a denied shell call `cd`-ing to the
/// matter, a Glob at the matter folder itself, Reads of other files, `stetdoc view` of the Word
/// files, Writes outside the matter; the lawyer's note lands mid-turn, then a Grep at the matter
/// root names it in its locations and output, and no write names it. The host flags it, before
/// and after this fix, and the default choice keeps it: the dry run's silent delete was the
/// thread's Undo ignoring the flag (`ui/app/src/room/thread/stop-rows.tsx`). This pins the flag.
#[test]
fn the_dry_runs_calls_leave_the_note_flagged() {
    let files = [
        ("ordinary.txt", "ordinary\n"),
        ("Main Agreement.docx", "agreement\n"),
        ("Notes.docx", "notes\n"),
    ];
    let (_dir, host, thread, turn, _folder, flags) = run_turn(
        "lawyer_file_dry_run",
        "undo-lawyer-file-run",
        &files,
        |folder| {
            write::write(&folder.join(NOTE), b"Call the client about clause 4.\n");
        },
    );
    assert_eq!(flags, [flag(NOTE, true)]);
    assert_eq!(edited_since(&host, thread, turn), Vec::<String>::new());
    // The agent globbed the root, the lawyer added the note, a folder-wide search named it and
    // no write did: the Undo dialogs keep it by default.
    assert_eq!(
        default_choices(&flags),
        [FileDecision {
            path: MatterPath::new(NOTE).unwrap(),
            choice: FileChoice::Keep,
        }]
    );
    host.shutdown();
}

/// Clauseo's own Word edits, `stetdoc` run through the agent's shell (Bash's default
/// `NAME.edited.docx`, PowerShell's `--in-place`): no location names either file, but the host
/// reads both writes from the commands (`shell_writes`, `stetdoc_args`), so neither change is
/// flagged and the thread's Undo does not ask; the default Undo takes both back.
#[test]
fn stetdocs_own_writes_through_the_shell_are_unflagged() {
    let files = [
        ("Main Agreement.docx", "agreement\n"),
        ("Notes.docx", "draft notes\n"),
    ];
    let (_dir, host, thread, turn, folder, flags) = run_turn(
        "lawyer_file_stetdoc",
        "undo-lawyer-file-stetdoc",
        &files,
        |folder| {
            let edited = folder.join("Main Agreement.edited.docx");
            write::write(&edited, b"agreement, with tracked changes\n");
            before_end(&edited);
            write::write(&folder.join("Notes.docx"), b"final notes\n");
        },
    );
    assert_eq!(
        flags,
        [
            flag("Main Agreement.edited.docx", false),
            flag("Notes.docx", false)
        ]
    );
    // No flagged file and nothing changed since: the thread's Undo undoes at once.
    assert_eq!(edited_since(&host, thread, turn), Vec::<String>::new());
    host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: default_choices(&flags),
    });
    assert!(!folder.join("Main Agreement.edited.docx").exists());
    assert_eq!(
        std::fs::read(folder.join("Notes.docx")).unwrap(),
        b"draft notes\n"
    );
    host.shutdown();
}

/// stetdoc's writes with the matter spelled through a symlink (macOS's `/var` for
/// `/private/var`, a matter under a linked cloud folder): the default output after
/// `cd "<link>"` and an absolute `-o "<link>/Reply.docx"` are resolved into the matter, so
/// neither is flagged.
#[cfg(unix)]
#[test]
fn stetdocs_writes_through_a_symlinked_spelling_are_unflagged() {
    let files = [("Main Agreement.docx", "agreement\n")];
    let (_dir, host, thread, turn, _folder, flags) = run_turn(
        "lawyer_file_stetdoc_link",
        "undo-lawyer-file-stetdoc-link",
        &files,
        |folder| {
            write::symlink(folder, &folder.parent().unwrap().join("m-link"));
            for name in ["Main Agreement.edited.docx", "Reply.docx"] {
                write::write(&folder.join(name), b"the agent's edit\n");
                before_end(&folder.join(name));
            }
        },
    );
    assert_eq!(
        flags,
        [
            flag("Main Agreement.edited.docx", false),
            flag("Reply.docx", false)
        ]
    );
    assert_eq!(edited_since(&host, thread, turn), Vec::<String>::new());
    host.shutdown();
}
