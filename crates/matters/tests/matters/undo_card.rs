//! Undoing only the papers on the answer's card (`UndoRequest::only`): another answer's change
//! in the same step (`Card::others`) is left as it is; a change the card cannot explain still
//! needs a decision; a change on the card the step holds otherwise refuses the undo.

use std::collections::BTreeSet;

use clauseo_matters::{Card, Error, Matter};
use clauseo_protocol::{ChangeKind, ErrorCode, FileChange, FileChoice, FileDecision, MatterPath};

use crate::fixture::{
    Fixture, added, assert_none, cid, deleted, meta, modified, path, renamed, tools, undo_all,
    undo_request,
};

/// Checkpoint 3 after a step that changed `a.docx`, `b.docx` and `c.docx`, with its changes;
/// checkpoint 1 before it.
fn stepped(name: &str) -> (Fixture, Matter, Vec<FileChange>) {
    let fx = Fixture::new(name);
    for name in ["a.docx", "b.docx", "c.docx"] {
        fx.write(name, "one");
    }
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    for name in ["a.docx", "b.docx", "c.docx"] {
        fx.write(name, "two");
    }
    let tool_paths = tools(&["a.docx", "b.docx", "c.docx"]);
    let changes = matter.after_turn(&meta(3), &tool_paths).unwrap().changes;
    (fx, matter, changes)
}

/// The changes of `changes` at `paths`: one answer's card, when another changed the rest.
fn on_card(changes: &[FileChange], paths: &[&str]) -> Vec<FileChange> {
    changes
        .iter()
        .filter(|change| paths.contains(&change.path().as_str()))
        .cloned()
        .collect()
}

fn paths(texts: &[&str]) -> BTreeSet<MatterPath> {
    texts.iter().map(|text| path(text)).collect()
}

#[test]
fn undo_puts_back_only_the_papers_on_the_card() {
    let (fx, mut matter, changes) = stepped("undo-card");
    let changes = on_card(&changes, &["a.docx", "b.docx"]);
    let others = paths(&["c.docx"]);
    let only = Card {
        changes: &changes,
        others: &others,
    };
    // Edited since the step: `c.docx` is another answer's, so not listed.
    fx.write("c.docx", "three");
    fx.write("a.docx", "three");
    assert_eq!(
        matter.edited_since(cid(1), cid(3), None).unwrap(),
        [path("a.docx"), path("c.docx")]
    );
    assert_eq!(
        matter.edited_since(cid(1), cid(3), Some(only)).unwrap(),
        [path("a.docx")]
    );

    // A decision naming `c.docx` is refused before anything is written.
    let decisions = undo_all(&["a.docx", "b.docx", "c.docx"]);
    let mut request = undo_request(1, 3, &decisions, 4, 5);
    request.only = Some(only);
    let err = matter.undo(&request).unwrap_err();
    assert!(
        matches!(&err, Error::UnexpectedDecision { path: p } if *p == path("c.docx")),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::CommandInvalid);
    assert_eq!(fx.read("a.docx"), b"three");
    assert_eq!(fx.read("b.docx"), b"two");
    assert_eq!(matter.timeline().len(), 2);

    // With no other answer to explain it, `c.docx` is part of the step as without a card: it
    // needs a decision, and its absence refuses the undo as today.
    let none = BTreeSet::new();
    let decisions = undo_all(&["a.docx", "b.docx"]);
    let mut request = undo_request(1, 3, &decisions, 4, 5);
    request.only = Some(Card {
        changes: &changes,
        others: &none,
    });
    let err = matter.undo(&request).unwrap_err();
    assert!(
        matches!(&err, Error::MissingDecision { path: p } if *p == path("c.docx")),
        "{err:?}"
    );
    assert_eq!(matter.timeline().len(), 2);

    let mut request = undo_request(1, 3, &decisions, 4, 5);
    request.only = Some(only);
    let undone = matter.undo(&request).unwrap();
    assert_eq!(undone.restored, [path("a.docx"), path("b.docx")]);
    assert_none(&undone.kept);
    assert_eq!(undone.edited_since, [path("a.docx")]);
    assert_eq!(fx.read("a.docx"), b"one");
    assert_eq!(fx.read("b.docx"), b"one");
    assert_eq!(fx.read("c.docx"), b"three");
    // The undo checkpoint records the two papers put back and nothing of `c.docx`.
    assert_eq!(
        undone.checkpoint.changes,
        [
            modified("a.docx", b"three", b"one"),
            modified("b.docx", b"two", b"one"),
        ]
    );
}

#[test]
fn a_rename_on_the_card_is_undone_whole() {
    let fx = Fixture::new("undo-card-rename");
    fx.write("old.docx", "moving bytes");
    fx.write("other.docx", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    crate::support::rename(fx.file("old.docx"), fx.file("new.docx"));
    fx.write("other.docx", "two");
    let tool_paths = tools(&["old.docx", "new.docx", "other.docx"]);
    let changes = matter.after_turn(&meta(3), &tool_paths).unwrap().changes;
    assert_eq!(
        changes,
        [
            renamed("old.docx", "new.docx", b"moving bytes"),
            modified("other.docx", b"one", b"two"),
        ]
    );

    let changes = on_card(&changes, &["new.docx"]);
    let others = paths(&["other.docx"]);
    let only = Card {
        changes: &changes,
        others: &others,
    };
    assert_none(&matter.edited_since(cid(1), cid(3), Some(only)).unwrap());
    let decisions = [FileDecision {
        path: path("new.docx"),
        choice: FileChoice::Undo,
    }];
    let mut request = undo_request(1, 3, &decisions, 4, 5);
    request.only = Some(only);
    let undone = matter.undo(&request).unwrap();
    assert_eq!(undone.restored, [path("new.docx")]);
    assert!(!fx.file("new.docx").exists());
    assert_eq!(fx.read("old.docx"), b"moving bytes");
    assert_eq!(fx.read("other.docx"), b"two");
}

/// The step's diff pairs a rename by content alone, the first unused deletion in path order:
/// two papers with the same bytes deleted and one added give `a.docx` → `b.docx`. A card that
/// renamed `z.docx` → `b.docx` (or added `b.docx`) is not that step: undoing its half would
/// delete `b.docx` and write back the wrong paper, or none. The undo is refused, nothing written.
#[test]
fn a_card_rename_the_step_splits_refuses_the_undo() {
    let fx = Fixture::new("undo-card-split");
    fx.write("a.docx", "same");
    fx.write("z.docx", "same");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    crate::support::remove_file(fx.file("a.docx"));
    crate::support::rename(fx.file("z.docx"), fx.file("b.docx"));
    let tool_paths = tools(&["a.docx", "b.docx", "z.docx"]);
    let step = matter.after_turn(&meta(3), &tool_paths).unwrap().changes;
    assert_eq!(
        step,
        [
            renamed("a.docx", "b.docx", b"same"),
            deleted("z.docx", b"same")
        ]
    );

    let pair = renamed("a.docx", "b.docx", b"same");
    let renamed_from_z = FileChange::new(
        path("b.docx"),
        ChangeKind::Renamed {
            from: path("z.docx"),
        },
        pair.before().cloned(),
        pair.after().cloned(),
        pair.size_bytes(),
    )
    .unwrap();
    let none = BTreeSet::new();
    for card in [
        vec![deleted("a.docx", b"same"), renamed_from_z],
        vec![added("b.docx", b"same")],
    ] {
        let only = Card {
            changes: &card,
            others: &none,
        };
        let err = matter.edited_since(cid(1), cid(3), Some(only)).unwrap_err();
        assert!(
            matches!(&err, Error::NotOnCard { path: p } if *p == path("b.docx")),
            "{err:?}"
        );
        let names: Vec<&str> = card.iter().map(|change| change.path().as_str()).collect();
        let decisions = undo_all(&names);
        let mut request = undo_request(1, 3, &decisions, 4, 5);
        request.only = Some(only);
        let err = matter.undo(&request).unwrap_err();
        assert!(
            matches!(&err, Error::NotOnCard { path: p } if *p == path("b.docx")),
            "{err:?}"
        );
        assert_eq!(err.code(), ErrorCode::CommandInvalid);
        assert_eq!(matter.timeline().len(), 2);
        assert!(!fx.file("a.docx").exists());
        assert!(!fx.file("z.docx").exists());
        assert_eq!(fx.read("b.docx"), b"same");
    }
}

/// The reviewer's case: a turn grew `big.docx` over the cap and copied its old bytes to
/// `copy.docx`. Its card (`after_turn`'s changes, the turn's `FilesChanged`, no `beside`)
/// has `copy.docx` added, while the step pairs it with `big.docx` left out: the card refuses
/// the undo. With no paper `beside` the host takes no card (`only` is `None`), so the undo is
/// today's: the copy is kept, the grown file left as it is.
#[test]
fn an_over_cap_copy_with_nothing_beside_undoes_as_today() {
    let mut fx = Fixture::new("undo-card-over-cap-copy");
    fx.limits.file_cap_bytes = 10;
    fx.write("big.docx", "small");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("big.docx", "far over the cap");
    fx.write("copy.docx", "small");
    let checkpoint = matter
        .after_turn(&meta(3), &tools(&["big.docx", "copy.docx"]))
        .unwrap();
    assert_eq!(checkpoint.skipped.len(), 1);
    assert_eq!(checkpoint.changes, [added("copy.docx", b"small")]);
    let changes = checkpoint.changes;
    let none = BTreeSet::new();
    let only = Card {
        changes: &changes,
        others: &none,
    };
    let decisions = undo_all(&["copy.docx"]);
    let mut request = undo_request(1, 3, &decisions, 4, 5);

    // The card would refuse it.
    let err = matter.edited_since(cid(1), cid(3), Some(only)).unwrap_err();
    assert!(matches!(&err, Error::NotOnCard { .. }), "{err:?}");
    request.only = Some(only);
    let err = matter.undo(&request).unwrap_err();
    assert!(matches!(&err, Error::NotOnCard { .. }), "{err:?}");
    assert_eq!(matter.timeline().len(), 2);

    // Without it, today's undo.
    request.only = None;
    assert_none(&matter.edited_since(cid(1), cid(3), None).unwrap());
    let undone = matter.undo(&request).unwrap();
    assert_none(&undone.restored);
    assert_eq!(undone.kept, [path("copy.docx")]);
    assert_eq!(fx.read("big.docx"), b"far over the cap");
    assert_eq!(fx.read("copy.docx"), b"small");
}
