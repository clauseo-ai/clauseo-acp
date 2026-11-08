//! Windows: a document Word holds open with no `~$` owner file beside it. Word opens a document
//! through a short (8.3) folder path when the full path is over 259 units, and then makes none,
//! so the no-share open is the only thing that shows it held: the check finds it, and an undo or
//! a restore that would write it is refused, the bytes unchanged.

use clauseo_matters::{Error, HeldOpen, RestoreFiles, held_open, held_open_at};
use clauseo_protocol::ErrorCode;

use crate::fixture::{Fixture, cid, meta, path, restore_request, tools, undo};
use crate::support::hold_as_word;

/// Matter 1 with `Invoice.docx` edited from `one` to `two` at checkpoint 3, and no owner file
/// beside it.
fn edited_invoice(name: &str) -> (Fixture, clauseo_matters::Matter) {
    let fx = Fixture::new(name);
    fx.write("Invoice.docx", "one");
    let mut matter = fx.import();
    fx.write("Invoice.docx", "two");
    matter
        .after_turn(&meta(3), &tools(&["Invoice.docx"]))
        .unwrap();
    (fx, matter)
}

/// Word holds `Invoice.docx` with no owner file beside it: `held_open` and `held_open_at` report
/// it as a sharing violation, and the restore preview names it locked. An undo that would write
/// it is refused with `matter.file_locked`, and the bytes are unchanged. Once Word lets go, the
/// undo goes through.
#[test]
fn an_undo_over_a_document_word_holds_with_no_owner_file_is_refused() {
    let (fx, mut matter) = edited_invoice("word-no-owner-undo");
    let held = hold_as_word(fx.file("Invoice.docx"));

    assert_eq!(
        held_open(&fx.folder, &path("Invoice.docx")),
        Some(HeldOpen::SharingViolation)
    );
    assert_eq!(
        held_open_at(&fx.folder, &fx.file("Invoice.docx")),
        Some((path("Invoice.docx"), HeldOpen::SharingViolation))
    );
    let plan = matter.preview_restore(cid(1), RestoreFiles::All).unwrap();
    assert_eq!(plan.locked, [path("Invoice.docx")], "{plan:?}");

    let err = undo(&mut matter, (1, 3), &["Invoice.docx"], (4, 5)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterFileLocked, "{err:?}");
    assert!(
        matches!(&err, Error::FileLocked { paths } if paths == &[path("Invoice.docx")]),
        "{err:?}"
    );
    assert_eq!(fx.read("Invoice.docx"), b"two");
    drop(held);

    let undone = undo(&mut matter, (1, 3), &["Invoice.docx"], (4, 5)).unwrap();
    assert_eq!(undone.restored, [path("Invoice.docx")]);
    assert_eq!(fx.read("Invoice.docx"), b"one");
}

/// The restore counterpart of the test above: the restore is refused with `matter.file_locked`
/// and writes nothing; once Word lets go, it goes through.
#[test]
fn a_restore_over_a_document_word_holds_with_no_owner_file_is_refused() {
    let (fx, mut matter) = edited_invoice("word-no-owner-restore");
    let held = hold_as_word(fx.file("Invoice.docx"));

    let err = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap_err();
    assert_eq!(err.code(), ErrorCode::MatterFileLocked, "{err:?}");
    assert!(
        matches!(&err, Error::FileLocked { paths } if paths == &[path("Invoice.docx")]),
        "{err:?}"
    );
    assert_eq!(fx.read("Invoice.docx"), b"two");
    drop(held);

    let restored = matter
        .restore(&restore_request(1, RestoreFiles::All, 4, 5))
        .unwrap();
    assert_eq!(restored.written, [path("Invoice.docx")]);
    assert_eq!(fx.read("Invoice.docx"), b"one");
}
