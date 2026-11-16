//! A `.clauseo/matter.json` an import finds: one naming the matter imported is adopted, kept
//! byte for byte whatever the import's end; one that does not read is set aside under a
//! stamped name and a new one written.

use std::sync::atomic::{AtomicBool, Ordering};

use clauseo_matters::{
    Error, Imported, Matter, NewMatter, Refusal, history_exists, read_marker, scan,
    set_aside_marker,
};

use crate::fixture::{Fixture, at, matter_id};
use crate::support;

/// A marker naming matter 1, as another install wrote it: its own time and spacing, and a key
/// this build does not know.
const FOREIGN: &str = concat!(
    r#"{ "version": 1, "matter_id": "01910000-0000-7000-8000-000000000001", "#,
    r#""created_at": 1600000000000, "written_by": "elsewhere" }"#
);

/// `at(0)` in UTC, as a set-aside marker's name carries it.
const STAMP: &str = "20231114T221320Z";

#[test]
fn a_marker_naming_the_matter_is_adopted_byte_for_byte() {
    let fx = Fixture::new("adopt-kept");
    fx.write("brief.txt", "Clause 4");
    fx.write(".clauseo/matter.json", FOREIGN);
    let cancel = AtomicBool::new(false);
    scan(&fx.folder, &[], &fx.settings(), &mut |_| {}, &cancel).unwrap();

    let matter = fx.import();
    assert_eq!(matter.id(), matter_id(1));
    assert_eq!(matter.timeline().len(), 1, "one import checkpoint");
    assert_eq!(fx.read(".clauseo/matter.json"), FOREIGN.as_bytes());
    drop(matter);
    let mut opened = fx.open(&fx.folder).unwrap();
    opened.before_turn(&crate::fixture::meta(2)).unwrap();
    assert_eq!(fx.read(".clauseo/matter.json"), FOREIGN.as_bytes());
}

#[test]
fn a_cancelled_or_discarded_adopting_import_leaves_the_marker() {
    let fx = Fixture::new("adopt-cancel");
    for name in ["a.txt", "b.txt"] {
        fx.write(name, name);
    }
    fx.write(".clauseo/matter.json", FOREIGN);
    let cancel = AtomicBool::new(false);
    let imported = Matter::import(
        fx.settings(),
        fx.new_matter(),
        &[],
        &mut |_| cancel.store(true, Ordering::Relaxed),
        &cancel,
    )
    .unwrap();
    assert!(matches!(imported, Imported::Cancelled));
    assert!(!fx.home().exists());
    assert_eq!(fx.read(".clauseo/matter.json"), FOREIGN.as_bytes());

    fx.import().discard().unwrap();
    assert!(!fx.home().exists());
    assert_eq!(fx.read(".clauseo/matter.json"), FOREIGN.as_bytes());
}

/// An import that fails on a file nobody may read (mode 000, so Unix only, and not as root).
#[cfg(unix)]
#[test]
fn a_failed_adopting_import_leaves_the_marker() {
    let fx = Fixture::new("adopt-failed");
    fx.write("held.txt", "x");
    fx.write(".clauseo/matter.json", FOREIGN);
    support::set_mode(fx.file("held.txt"), 0o000);
    let reads_anyway = std::fs::read(fx.file("held.txt")).is_ok();
    let cancel = AtomicBool::new(false);
    let result = Matter::import(fx.settings(), fx.new_matter(), &[], &mut |_| {}, &cancel);
    support::set_mode(fx.file("held.txt"), 0o644);
    if reads_anyway {
        return;
    }
    assert!(result.is_err(), "{result:?}");
    assert!(!fx.home().exists());
    assert_eq!(fx.read(".clauseo/matter.json"), FOREIGN.as_bytes());
}

#[test]
fn a_marker_that_does_not_read_is_set_aside_and_a_new_one_written() {
    let fx = Fixture::new("adopt-aside");
    fx.write("a.txt", "a");
    fx.write(".clauseo/matter.json", "{not json");
    let cancel = AtomicBool::new(false);
    scan(&fx.folder, &[], &fx.settings(), &mut |_| {}, &cancel).unwrap();
    assert_eq!(read_marker(&fx.folder).unwrap(), None);

    let matter = fx.import();
    assert_eq!(read_marker(&fx.folder).unwrap(), Some(matter_id(1)));
    let aside = format!(".clauseo/matter.old-{STAMP}.json");
    assert_eq!(fx.read(&aside), b"{not json");
    // `.clauseo` is never versioned: the papers are the one file.
    assert_eq!(matter.timeline().len(), 1);
    drop(matter);
    fx.open(&fx.folder).unwrap();

    // A second one in the same second takes `-2`.
    fx.write(".clauseo/matter.json", "also broken");
    let second = set_aside_marker(&fx.folder, at(0)).unwrap();
    assert_eq!(
        second,
        fx.file(&format!(".clauseo/matter.old-{STAMP}-2.json"))
    );
    assert_eq!(std::fs::read(second).unwrap(), b"also broken");
    assert!(!fx.exists(".clauseo/matter.json"));
    assert_eq!(fx.read(&aside), b"{not json");
}

/// A folder refused for its place keeps a marker that does not read as it was.
#[test]
fn a_refused_folder_keeps_its_unreadable_marker() {
    let fx = Fixture::new("adopt-aside-refused");
    let data = fx.dir.path().join("data");
    let mut settings = fx.settings();
    settings.refused_trees = vec![data.clone()];
    let folder = data.join("x");
    support::write(folder.join(".clauseo/matter.json"), "{not json");
    let new = NewMatter {
        folder: folder.clone(),
        ..fx.new_matter()
    };
    let err = Matter::import(settings, new, &[], &mut |_| {}, &AtomicBool::new(false)).unwrap_err();
    assert!(
        matches!(
            err,
            Error::FolderRefused {
                reason: Refusal::Listed(_),
                ..
            }
        ),
        "{err:?}"
    );
    let bytes = std::fs::read(folder.join(".clauseo/matter.json")).unwrap();
    assert_eq!(bytes, b"{not json");
}

/// `history_exists` says whether a matter's history is under the matters root; an import under
/// an id that has one is refused before anything is written, its marker set aside or not.
#[test]
fn history_exists_answers_for_the_matters_root() {
    let fx = Fixture::new("adopt-history");
    fx.write("a.txt", "a");
    assert!(!history_exists(&fx.settings(), matter_id(1)).unwrap());
    drop(fx.import());
    assert!(history_exists(&fx.settings(), matter_id(1)).unwrap());
    assert!(!history_exists(&fx.settings(), matter_id(2)).unwrap());

    let other = fx.dir.path().join("other");
    support::write(other.join(".clauseo/matter.json"), "{not json");
    let new = NewMatter {
        folder: other.clone(),
        ..fx.new_matter()
    };
    let err = Matter::import(
        fx.settings(),
        new,
        &[],
        &mut |_| {},
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(matches!(err, Error::HistoryExists { .. }), "{err:?}");
    let bytes = std::fs::read(other.join(".clauseo/matter.json")).unwrap();
    assert_eq!(bytes, b"{not json");
}
