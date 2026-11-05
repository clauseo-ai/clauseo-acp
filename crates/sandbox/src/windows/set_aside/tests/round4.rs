//! Round 4 of the S15 `PR3b` review: two names that differ only in case are each checked on
//! their own, and a name that isn't text is recorded like any other.

use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;
use std::path::Path;

use super::{exit, key, launch, volume, world};
use crate::windows::set_aside::check::{check, snapshot};
use crate::windows::set_aside::files::{FileId, Place, Record, Snapshot};
use crate::windows::set_aside::run::Volume as _;
use crate::windows::set_aside::scan::{Item, Kept, Now, Scan};
use crate::windows::set_aside::{Entry, Reason};
use crate::windows::test_world::matter_one;
use crate::windows::write::rename_new;

/// The file ID of `path`, which must be there.
fn id(path: &Path) -> FileId {
    volume().file_id(path).unwrap().unwrap()
}

/// The scan's item for the nested `.git` keyed `<folder>/.git`, whose folder is `on_disk` in
/// the matter `root`.
fn nested_git(root: &Path, folder: &str, on_disk: &str) -> Item {
    Item {
        key: key(&format!("{folder}/.git")),
        path: root.join(on_disk).join(".git"),
        within: vec![OsString::from(folder)],
        through: vec![id(&root.join(on_disk))],
        now: Now::At(id(&root.join(on_disk).join(".git"))),
        kept: Kept::Nested,
    }
}

/// Round 4, the case-sensitive folder escape: `inside()` matched keys ignoring case on
/// Windows, so once `A\.git` was set aside, an `a\.git` beside it in a case-sensitive folder
/// was passed over and left out of the record. Each is now checked and recorded on its own.
/// The scan is built by hand: the stand-in's temporary folder may ignore case, so `a` lives
/// at another path on disk, and its key differs from `A`'s only in case.
#[test]
fn a_git_differing_only_in_case_from_one_set_aside_is_still_checked_and_recorded() {
    let (w, root, data) = world("aside-case-pair");
    w.dir.file("matter/A/.git/HEAD", "ref: refs/heads/main\n");
    w.dir
        .file("matter/lower/.git/HEAD", "ref: refs/heads/main\n");
    let scan = Scan {
        items: vec![nested_git(&root, "A", "A"), nested_git(&root, "a", "lower")],
        unscanned: Vec::new(),
    };
    let lower = id(&root.join("lower/.git"));
    let seen = Snapshot {
        names: BTreeMap::from([(key("a/.git"), Some(lower))]),
        links: BTreeMap::new(),
    };
    let record = Record::new(matter_one(), BTreeSet::from(["s1".to_owned()]), seen);
    let place = Place::new(&data, &matter_one());
    let held = place.lock().unwrap();

    let mut moved = Vec::new();
    let aside = check(&volume(), &place, &scan, &record, launch(), &mut moved).unwrap();
    let from: Vec<_> = moved.iter().map(|e| e.from.clone()).collect();
    assert_eq!(from, [root.join("A/.git")]);
    let after = snapshot(&scan, &aside).unwrap();
    assert_eq!(after.names.get(&key("a/.git")), Some(&Some(lower)));
    assert!(!after.names.contains_key(&key("A/.git")));

    // A new `a\.git` beside a new `A\.git` is set aside too, not passed over.
    w.dir.file("matter/A/.git/HEAD", "ref: refs/heads/main\n");
    rename_new(&root.join("lower/.git"), &root.join("lower/old")).unwrap();
    w.dir
        .file("matter/lower/.git/HEAD", "ref: refs/heads/main\n");
    let scan = Scan {
        items: vec![nested_git(&root, "A", "A"), nested_git(&root, "a", "lower")],
        unscanned: Vec::new(),
    };
    let mut moved = Vec::new();
    check(&volume(), &place, &scan, &record, exit(), &mut moved).unwrap();
    let seen: Vec<_> = moved.iter().map(|e| (e.from.clone(), e.reason)).collect();
    assert_eq!(
        seen,
        [
            (root.join("A/.git"), Reason::New),
            (root.join("lower/.git"), Reason::Changed),
        ]
    );
    drop(held);
}

/// Round 4, a name that isn't text: a link named with a lone surrogate failed every check.
/// Names are kept raw, and the record and the manifest write one that isn't text as its UTF-16
/// code units, which read back to the same name. (WTF-8 bytes stand for it here: on Windows
/// the name is UTF-16 itself.)
#[test]
fn a_name_with_a_lone_surrogate_is_written_and_read_back_unchanged() {
    let lone = OsString::from_vec(b"J\xED\xA0\x80".to_vec());
    let file = FileId {
        volume: 7,
        id: [1; 16],
    };
    let seen = Snapshot {
        names: BTreeMap::from([(vec![OsString::from("sub"), lone.clone()], Some(file))]),
        links: BTreeMap::from([(vec![lone.clone()], file)]),
    };
    let record = Record::new(matter_one(), BTreeSet::new(), seen);
    let json = serde_json::to_string(&record).unwrap();
    assert!(json.contains(r#"["sub",{"utf16":[74,55296]}]"#), "{json}");
    assert_eq!(serde_json::from_str::<Record>(&json).unwrap(), record);

    let (_w, _root, data) = world("aside-lone-surrogate");
    let place = Place::new(&data, &matter_one());
    let _held = place.lock().unwrap();
    place.write_record(&record).unwrap();
    assert_eq!(place.read_record().unwrap(), Some(record));

    let entry = Entry {
        id: 1,
        matter: matter_one(),
        from: Path::new("/m/sub").join(&lone),
        to: Path::new("/d/pass/sub").join(&lone),
        folder: "20251105T111500-123Z".to_owned(),
        within: vec![OsString::from("sub")],
        name: lone,
        reason: Reason::New,
        owner: None,
        at: "2025-11-05T11:15:00.123Z".to_owned(),
    };
    let json = serde_json::to_string(&entry).unwrap();
    assert_eq!(serde_json::from_str::<Entry>(&json).unwrap(), entry);

    // Bytes that aren't WTF-8 have no UTF-16 form: only the stand-in can hold them.
    let bad = Snapshot {
        names: BTreeMap::from([(vec![OsString::from_vec(vec![0xFF])], None)]),
        links: BTreeMap::new(),
    };
    assert!(serde_json::to_string(&Record::new(matter_one(), BTreeSet::new(), bad)).is_err());
}
