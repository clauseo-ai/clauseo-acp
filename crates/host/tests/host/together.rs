//! The cards of conversations whose answers ran at the same time in one matter: each card holds
//! its own papers, measured from its own start; a paper both wrote is on both cards, each naming
//! the other conversation in `also_changed_by`; undo of one puts back only its own papers. Two
//! answers one after the other get the cards they always got.

use std::path::Path;

use clauseo_protocol::{
    FileChoice, FileDecision, FilesChanged, HostCommand, HostQuery, MatterPath, QueryResult,
    StopReason, StreamId, ThreadId, TurnId,
};

use crate::observe::Events;
use crate::permissions::{answer_from, until_first_request};
use crate::support::{Dir, Running, Scenario};

/// Writes `notes/a.txt` ("Alpha"), then waits on `perm-1`.
const A: &str = "a-agent";
/// Writes `notes/b.txt` ("Beta"), then ends.
const B_OWN: &str = "b-own-agent";
/// Writes `notes/a.txt` ("Beta"), then ends.
const B_SAME: &str = "b-same-agent";

fn dir(test: &str) -> Dir {
    Dir::new(
        test,
        &[
            (A, Scenario::Bundled("write-then-wait")),
            (B_OWN, Scenario::Bundled("write-note-b")),
            (B_SAME, Scenario::Bundled("rewrite-note-a")),
        ],
    )
}

/// A matter with a brief, thread A and a thread B playing `b`.
struct Matter {
    folder: std::path::PathBuf,
    a: ThreadId,
    b: ThreadId,
    events_a: Events,
    events_b: Events,
}

fn matter(dir: &Dir, host: &Running, b: &str) -> Matter {
    let folder = dir.folder("m", &[("brief.txt", "Clause 4 draft\n")]);
    let matter = host.create_matter("m", &folder);
    let a = host.create_thread(matter, A);
    let b = host.create_thread(matter, b);
    Matter {
        folder,
        a,
        b,
        events_a: host.events(StreamId::Thread(a)),
        events_b: host.events(StreamId::Thread(b)),
    }
}

/// Each change of `turn`'s card: its path, whether it may hold the lawyer's edits, and the
/// other conversations that also changed it.
fn card(events: &Events, turn: TurnId) -> Vec<(String, bool, Vec<ThreadId>)> {
    let changed: FilesChanged = events
        .files_changed(turn)
        .unwrap_or_else(|| panic!("no FilesChanged: {}", events.dump()));
    changed
        .files
        .iter()
        .map(|file| {
            (
                file.path().as_str().to_owned(),
                file.may_include_lawyer_edits(),
                file.also_changed_by().to_vec(),
            )
        })
        .collect()
}

fn read(folder: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(folder.join(name)).ok()
}

fn undo(host: &Running, thread: ThreadId, turn: TurnId, path: &str) {
    host.ok(HostCommand::UndoStep {
        thread,
        turn,
        files: vec![FileDecision {
            path: MatterPath::new(path).unwrap(),
            choice: FileChoice::Undo,
        }],
    });
    host.settle();
}

/// A starts and writes notes/a.txt, B's whole turn runs while A waits, then A ends.
fn overlapping(host: &Running, m: &mut Matter) -> (TurnId, TurnId) {
    let (turn_a, _) = host.prompt(m.a, "Write a note.");
    until_first_request(&mut m.events_a);
    let (turn_b, _) = host.prompt(m.b, "Write a note too.");
    let ended_b = m.events_b.until_turn_ended(turn_b);
    assert_eq!(ended_b.stop, StopReason::EndTurn, "{}", m.events_b.dump());
    answer_from(host, &mut m.events_a, m.a, 0, &["allow-once"]);
    let ended_a = m.events_a.until_turn_ended(turn_a);
    assert_eq!(ended_a.stop, StopReason::EndTurn, "{}", m.events_a.dump());
    host.settle();
    (turn_a, turn_b)
}

#[test]
fn overlapping_answers_on_different_papers_each_card_holds_its_own() {
    let dir = dir("together_different_papers");
    let host = dir.start(0);
    let mut m = matter(&dir, &host, B_OWN);
    let (turn_a, turn_b) = overlapping(&host, &mut m);

    assert_eq!(
        card(&m.events_a, turn_a),
        [("notes/a.txt".to_owned(), false, vec![])]
    );
    assert_eq!(
        card(&m.events_b, turn_b),
        [("notes/b.txt".to_owned(), false, vec![])]
    );

    // Undo of A puts back A's paper and leaves B's as B left it.
    undo(&host, m.a, turn_a, "notes/a.txt");
    assert_eq!(read(&m.folder, "notes/a.txt"), None);
    assert_eq!(read(&m.folder, "notes/b.txt").as_deref(), Some("Beta\n"));
    m.events_a.assert_no_mismatch();
    m.events_b.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn overlapping_answers_on_the_same_paper_both_cards_name_the_other() {
    let dir = dir("together_same_paper");
    let host = dir.start(0);
    let mut m = matter(&dir, &host, B_SAME);
    let (turn_a, turn_b) = overlapping(&host, &mut m);
    assert_eq!(read(&m.folder, "notes/a.txt").as_deref(), Some("Beta\n"));

    assert_eq!(
        card(&m.events_a, turn_a),
        [("notes/a.txt".to_owned(), false, vec![m.b])]
    );
    assert_eq!(
        card(&m.events_b, turn_b),
        [("notes/a.txt".to_owned(), false, vec![m.a])]
    );

    // The preview answers for A: the paper is as A's end left it.
    let preview = host.query(HostQuery::PreviewUndo {
        thread: m.a,
        turn: turn_a,
    });
    let Ok(QueryResult::UndoPreview(preview)) = preview else {
        panic!("PreviewUndo answered {preview:?}");
    };
    assert_eq!(preview.edited_since, Vec::<MatterPath>::new());
    // Undo of A puts back A's start: no notes/a.txt.
    undo(&host, m.a, turn_a, "notes/a.txt");
    assert_eq!(read(&m.folder, "notes/a.txt"), None);
    assert_eq!(
        read(&m.folder, "brief.txt").as_deref(),
        Some("Clause 4 draft\n")
    );
    m.events_a.assert_no_mismatch();
    m.events_b.assert_no_mismatch();
    host.shutdown();
}

#[test]
fn answers_one_after_the_other_get_the_cards_they_always_got() {
    let dir = dir("together_no_overlap");
    let host = dir.start(0);
    let mut m = matter(&dir, &host, B_SAME);
    let (turn_a, _) = host.prompt(m.a, "Write a note.");
    answer_from(&host, &mut m.events_a, m.a, 0, &["allow-once"]);
    m.events_a.until_turn_ended(turn_a);
    host.settle();
    let (turn_b, _) = host.prompt(m.b, "Write a note too.");
    m.events_b.until_turn_ended(turn_b);
    host.settle();

    // Each measured from the last checkpoint, with no `also_changed_by`: B's card holds the
    // paper A wrote before it started as B's own modification.
    assert_eq!(
        card(&m.events_a, turn_a),
        [("notes/a.txt".to_owned(), false, vec![])]
    );
    assert_eq!(
        card(&m.events_b, turn_b),
        [("notes/a.txt".to_owned(), false, vec![])]
    );
    m.events_a.assert_no_mismatch();
    m.events_b.assert_no_mismatch();
    host.shutdown();
}
