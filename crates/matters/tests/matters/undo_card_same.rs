//! While turns run one at a time the answer's card is every change of its step: an undo with
//! the card (`UndoRequest::only`, taken from `after_turn`'s changes as the turn's
//! `FilesChanged` holds them) does exactly what an undo without it does, success or the same
//! refusal, to the same files. Each case builds the same matter twice and undoes it both ways.

use std::collections::BTreeSet;
use std::path::Path;

use clauseo_matters::{Card, CheckpointOutcome, Error, Matter, Undone};
use clauseo_protocol::{FileChange, FileChoice, FileDecision};

use crate::fixture::{Fixture, assert_none, cid, meta, path, tools};

/// Each paper under `dir` (`.clauseo` aside), by its path, with its bytes.
fn papers(dir: &Path, prefix: &str, out: &mut Vec<(String, Vec<u8>)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if prefix.is_empty() && name == ".clauseo" {
            continue;
        }
        let rel = format!("{prefix}{name}");
        if entry.file_type().unwrap().is_dir() {
            papers(&entry.path(), &format!("{rel}/"), out);
        } else {
            out.push((rel, std::fs::read(entry.path()).unwrap()));
        }
    }
}

fn folder(fx: &Fixture) -> Vec<(String, Vec<u8>)> {
    let mut out = Vec::new();
    papers(&fx.folder, "", &mut out);
    out.sort();
    out
}

/// `result` as text with each commit id blanked: the two copies, made a moment apart, can
/// differ in what their commits record of the files' times, never in what the undo did.
fn no_commits(result: &Result<Undone, Error>) -> String {
    let text = format!("{result:?}");
    let mut out = String::with_capacity(text.len());
    let mut rest = text.as_str();
    while let Some(at) = rest.find("CommitId(\"") {
        let (head, tail) = rest.split_at(at + "CommitId(\"".len());
        out.push_str(head);
        let end = tail.find('"').unwrap();
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Every path of `card` decided `Undo`, as the host fills them when the lawyer names none.
fn undo_card(card: &[FileChange]) -> Vec<FileDecision> {
    card.iter()
        .map(|change| FileDecision {
            path: change.path().clone(),
            choice: FileChoice::Undo,
        })
        .collect()
}

/// Builds the matter twice with `build` (which returns the matter and the turn's card), then
/// checks that `edited_since` and the undo of checkpoint 1 to 3 with `decisions` (the card's
/// paths, all `Undo`, when `None`) come out the same with the card and without it, and that
/// the two folders end byte-identical. Returns the undo's result.
fn same(
    name: &str,
    build: impl Fn(&mut Fixture) -> (Matter, Vec<FileChange>),
    decisions: Option<&[FileDecision]>,
) -> Result<Undone, Error> {
    let mut with_fx = Fixture::new(&format!("{name}-card"));
    let mut without_fx = Fixture::new(&format!("{name}-none"));
    let (mut with, card) = build(&mut with_fx);
    let (mut without, _) = build(&mut without_fx);
    let none = BTreeSet::new();
    let only = Card {
        changes: &card,
        others: &none,
    };
    let filled = undo_card(&card);
    let decisions = decisions.unwrap_or(&filled);

    let edited_with = with.edited_since(cid(1), cid(3), Some(only));
    let edited_without = without.edited_since(cid(1), cid(3), None);
    assert_eq!(format!("{edited_with:?}"), format!("{edited_without:?}"));

    let mut request = crate::fixture::undo_request(1, 3, decisions, 4, 5);
    let undone_without = without.undo(&request);
    request.only = Some(only);
    let undone_with = with.undo(&request);
    assert_eq!(no_commits(&undone_with), no_commits(&undone_without));
    assert_eq!(folder(&with_fx), folder(&without_fx));
    assert_eq!(with.timeline().len(), without.timeline().len());
    undone_with
}

/// `mod.docx` modified, `new.docx` added, `gone.docx` deleted, `old.docx` renamed to
/// `moved.docx`; `mod.docx` was not named by a tool call (flagged as possibly the lawyer's)
/// and the lawyer edited it again after the turn.
fn whole_step(fx: &mut Fixture) -> (Matter, Vec<FileChange>) {
    fx.write("mod.docx", "one");
    fx.write("gone.docx", "doomed");
    fx.write("old.docx", "moving bytes");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("mod.docx", "two");
    fx.write("new.docx", "fresh");
    crate::support::remove_file(fx.file("gone.docx"));
    crate::support::rename(fx.file("old.docx"), fx.file("moved.docx"));
    let tool_paths = tools(&["new.docx", "gone.docx", "old.docx", "moved.docx"]);
    let card = matter.after_turn(&meta(3), &tool_paths).unwrap().changes;
    assert_eq!(card.len(), 4);
    fx.write("mod.docx", "three");
    (matter, card)
}

#[test]
fn only_the_whole_card_undoes_as_without() {
    let undone = same("card-same-whole", whole_step, None).unwrap();
    assert_eq!(undone.edited_since, [path("mod.docx")]);
}

/// (c) The flagged change is on the card like any other.
#[test]
fn a_flagged_lawyer_edit_undoes_as_without() {
    let undone = same(
        "card-same-flagged",
        |fx| {
            let (matter, card) = whole_step(fx);
            let flagged: Vec<&str> = card
                .iter()
                .filter(|change| change.may_include_lawyer_edits())
                .map(|change| change.path().as_str())
                .collect();
            assert_eq!(flagged, ["mod.docx"]);
            (matter, card)
        },
        None,
    )
    .unwrap();
    assert!(undone.restored.contains(&path("mod.docx")));
}

/// (f) Keep for some files and Undo for others; and a decision left out.
#[test]
fn keep_and_partial_decisions_undo_as_without() {
    let decide = |text: &str, choice| FileDecision {
        path: path(text),
        choice,
    };
    let mixed = [
        decide("gone.docx", FileChoice::Keep),
        decide("mod.docx", FileChoice::Keep),
        decide("moved.docx", FileChoice::Undo),
        decide("new.docx", FileChoice::Undo),
    ];
    let undone = same("card-same-keep", whole_step, Some(&mixed)).unwrap();
    assert_eq!(undone.restored, [path("moved.docx"), path("new.docx")]);
    let partial = &mixed[1..];
    let err = same("card-same-partial", whole_step, Some(partial)).unwrap_err();
    assert!(matches!(err, Error::MissingDecision { .. }), "{err:?}");
}

/// (h) The host's repair logs the card from `Matter::changes_between` (before-turn to the
/// recorded id), each change flagged.
#[test]
fn the_repaired_card_undoes_as_without() {
    same(
        "card-same-repair",
        |fx| {
            let (matter, _) = whole_step(fx);
            let card = matter
                .changes_between(cid(1), cid(3))
                .unwrap()
                .into_iter()
                .map(FileChange::with_lawyer_edits)
                .collect();
            (matter, card)
        },
        None,
    )
    .unwrap();
}

/// (a) A versioned file the turn grew over the cap: the card has it skipped, the step deleted.
/// Shrunk again afterwards, it is no longer left out of the step, so with the card as without
/// it the undo is refused for want of a decision for it.
#[test]
fn a_file_grown_over_the_cap_then_shrunk_undoes_as_without() {
    let result = same(
        "card-same-cap",
        |fx| {
            fx.limits.file_cap_bytes = 10;
            fx.write("big.docx", "small");
            fx.write("memo.docx", "one");
            let mut matter = fx.import();
            matter.before_turn(&meta(2)).unwrap();
            fx.write("big.docx", "far over the cap");
            fx.write("memo.docx", "two");
            let checkpoint = matter
                .after_turn(&meta(3), &tools(&["big.docx", "memo.docx"]))
                .unwrap();
            assert_eq!(checkpoint.skipped.len(), 1);
            fx.write("big.docx", "tiny");
            (matter, checkpoint.changes)
        },
        None,
    );
    let err = result.unwrap_err();
    assert!(
        matches!(&err, Error::MissingDecision { path: p } if *p == path("big.docx")),
        "{err:?}"
    );
}

/// (b) A cloud-only file the turn read for the first time, changed: on the card, kept by undo
/// as one with no earlier copy.
#[test]
fn a_first_read_undoes_as_without() {
    let undone = same(
        "card-same-first-read",
        |fx| {
            fx.write("brief.txt", "draft");
            fx.write("notes.txt", "the lawyer's notes");
            fx.probe.mark(fx.file("notes.txt"));
            let mut matter = fx.import();
            matter.before_turn(&meta(2)).unwrap();
            fx.probe.unmark(fx.file("notes.txt"));
            fx.write("notes.txt", "the agent's notes");
            fx.write("brief.txt", "final");
            let checkpoint = matter
                .after_turn(&meta(3), &tools(&["notes.txt", "brief.txt"]))
                .unwrap();
            (matter, checkpoint.changes)
        },
        None,
    )
    .unwrap();
    assert_eq!(undone.restored, [path("brief.txt")]);
}

/// (d) A turn that changed nothing: `after_turn` reuses the before-turn checkpoint, and the
/// card is empty.
#[test]
fn a_reused_checkpoint_undoes_as_without() {
    let result = same(
        "card-same-reused",
        |fx| {
            fx.write("brief.txt", "draft");
            let mut matter = fx.import();
            matter.before_turn(&meta(2)).unwrap();
            let checkpoint = matter.after_turn(&meta(3), &tools(&[])).unwrap();
            assert!(matches!(checkpoint.outcome, CheckpointOutcome::Reused(_)));
            (matter, checkpoint.changes)
        },
        None,
    );
    // Checkpoint 3 was never made: both ways refuse it alike.
    let err = result.unwrap_err();
    assert!(matches!(err, Error::UnknownCheckpoint { .. }), "{err:?}");
}

/// (e) A case-only rename.
#[test]
fn a_case_only_rename_undoes_as_without() {
    let undone = same(
        "card-same-case",
        |fx| {
            fx.write("Report.docx", "the report");
            let mut matter = fx.import();
            matter.before_turn(&meta(2)).unwrap();
            crate::support::rename(fx.file("Report.docx"), fx.file("report.docx"));
            let tool_paths = tools(&["Report.docx", "report.docx"]);
            let card = matter.after_turn(&meta(3), &tool_paths).unwrap().changes;
            (matter, card)
        },
        None,
    )
    .unwrap();
    assert_eq!(undone.restored, [path("report.docx")]);
}

/// (g) The empty card: the turn only added a file over the cap, so the card has no files and
/// one skipped.
#[test]
fn the_empty_card_undoes_as_without() {
    let undone = same(
        "card-same-empty",
        |fx| {
            fx.limits.file_cap_bytes = 10;
            fx.write("brief.txt", "draft");
            let mut matter = fx.import();
            matter.before_turn(&meta(2)).unwrap();
            fx.write("huge.docx", "far over the cap");
            let checkpoint = matter.after_turn(&meta(3), &tools(&["huge.docx"])).unwrap();
            assert_eq!(checkpoint.skipped.len(), 1);
            assert_none(&checkpoint.changes);
            (matter, checkpoint.changes)
        },
        None,
    );
    let undone = undone.unwrap();
    assert_none(&undone.restored);
}
