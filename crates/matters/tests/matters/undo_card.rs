//! Undoing only the papers on the answer's card (`UndoRequest::only`): another change in the
//! same step, made meanwhile by another answer, is left as it is.

use std::collections::BTreeSet;

use clauseo_matters::{Error, Matter};
use clauseo_protocol::{ErrorCode, FileChoice, FileDecision, MatterPath};

use crate::fixture::{
    Fixture, assert_none, cid, created, meta, modified, path, renamed, tools, undo_all,
    undo_request,
};

/// Checkpoint 3 after a step that changed `a.docx`, `b.docx` and `c.docx`; checkpoint 1
/// before it.
fn stepped(name: &str) -> (Fixture, Matter) {
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
    created(matter.after_turn(&meta(3), &tool_paths).unwrap(), 3);
    (fx, matter)
}

fn card(paths: &[&str]) -> BTreeSet<MatterPath> {
    paths.iter().map(|text| path(text)).collect()
}

#[test]
fn undo_puts_back_only_the_papers_on_the_card() {
    let (fx, mut matter) = stepped("undo-card");
    let only = card(&["a.docx", "b.docx"]);
    // Edited since the step: `c.docx` is not on the card, so not listed.
    fx.write("c.docx", "three");
    fx.write("a.docx", "three");
    assert_eq!(
        matter.edited_since(cid(1), cid(3), None).unwrap(),
        [path("a.docx"), path("c.docx")]
    );
    assert_eq!(
        matter.edited_since(cid(1), cid(3), Some(&only)).unwrap(),
        [path("a.docx")]
    );

    // A decision naming `c.docx` is refused before anything is written.
    let decisions = undo_all(&["a.docx", "b.docx", "c.docx"]);
    let mut request = undo_request(1, 3, &decisions, 4, 5);
    request.only = Some(&only);
    let err = matter.undo(&request).unwrap_err();
    assert!(
        matches!(&err, Error::UnexpectedDecision { path: p } if *p == path("c.docx")),
        "{err:?}"
    );
    assert_eq!(err.code(), ErrorCode::CommandInvalid);
    assert_eq!(fx.read("a.docx"), b"three");
    assert_eq!(fx.read("b.docx"), b"two");
    assert_eq!(matter.timeline().len(), 2);

    // A missing decision for a paper on the card is refused too.
    let decisions = undo_all(&["a.docx"]);
    let mut request = undo_request(1, 3, &decisions, 4, 5);
    request.only = Some(&only);
    let err = matter.undo(&request).unwrap_err();
    assert!(
        matches!(&err, Error::MissingDecision { path: p } if *p == path("b.docx")),
        "{err:?}"
    );
    assert_eq!(matter.timeline().len(), 2);

    let decisions = undo_all(&["a.docx", "b.docx"]);
    let mut request = undo_request(1, 3, &decisions, 4, 5);
    request.only = Some(&only);
    let undone = matter.undo(&request).unwrap();
    assert_eq!(undone.restored, [path("a.docx"), path("b.docx")]);
    assert_none(&undone.kept);
    assert_eq!(undone.edited_since, [path("a.docx")]);
    assert_eq!(fx.read("a.docx"), b"one");
    assert_eq!(fx.read("b.docx"), b"one");
    assert_eq!(fx.read("c.docx"), b"three");
    // The undo checkpoint records the two papers put back and nothing of `c.docx`.
    assert_eq!(
        created(undone.checkpoint, 5),
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
    assert_eq!(
        created(matter.after_turn(&meta(3), &tool_paths).unwrap(), 3),
        [
            renamed("old.docx", "new.docx", b"moving bytes"),
            modified("other.docx", b"one", b"two"),
        ]
    );

    let only = card(&["new.docx"]);
    assert_none(&matter.edited_since(cid(1), cid(3), Some(&only)).unwrap());
    let decisions = [FileDecision {
        path: path("new.docx"),
        choice: FileChoice::Undo,
    }];
    let mut request = undo_request(1, 3, &decisions, 4, 5);
    request.only = Some(&only);
    let undone = matter.undo(&request).unwrap();
    assert_eq!(undone.restored, [path("new.docx")]);
    assert!(!fx.file("new.docx").exists());
    assert_eq!(fx.read("old.docx"), b"moving bytes");
    assert_eq!(fx.read("other.docx"), b"two");
}
