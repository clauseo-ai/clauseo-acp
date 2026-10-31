//! Word's owner file for a 7-character stem drops one character: `~$nvoice.docx` beside an
//! open `Invoice.docx` (research 38, word-lock.json `name_lengths`, private #114). Undo and
//! restore both refuse through `locks::locked`; one test each, so neither path loses the check.

use clauseo_matters::{Error, Matter, RestoreFiles};

use crate::fixture::{Fixture, cid, created, meta, path, restore_request, tools, undo};

/// `Invoice.docx` "one" at checkpoint 1, "two" at checkpoint 3, and Word's owner file beside it.
fn open_in_word(name: &str) -> (Fixture, Matter) {
    let fx = Fixture::new(name);
    fx.write("Invoice.docx", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("Invoice.docx", "two");
    created(
        matter
            .after_turn(&meta(3), &tools(&["Invoice.docx"]))
            .unwrap(),
        3,
    );
    fx.write("~$nvoice.docx", "owner file: open in Word");
    (fx, matter)
}

fn assert_locked(err: &Error) {
    assert!(
        matches!(err, Error::FileLocked { paths } if *paths == [path("Invoice.docx")]),
        "{err:?}"
    );
}

#[test]
fn undo_refuses_seven_character_word_name() {
    let (fx, mut matter) = open_in_word("owner-undo");
    let err = undo(&mut matter, (1, 3), &["Invoice.docx"], (4, 5)).unwrap_err();
    assert_locked(&err);
    assert_eq!(fx.read("Invoice.docx"), b"two");
    assert_eq!(matter.last_checkpoint(), Some(cid(3)));
}

#[test]
fn restore_refuses_seven_character_word_name() {
    let (fx, mut matter) = open_in_word("owner-restore");
    let preview = matter.preview_restore(cid(1), RestoreFiles::All).unwrap();
    assert_eq!(preview.locked, [path("Invoice.docx")]);
    let err = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap_err();
    assert_locked(&err);
    assert_eq!(fx.read("Invoice.docx"), b"two");
    assert_eq!(matter.last_checkpoint(), Some(cid(3)));
}
