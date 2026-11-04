use clauseo_protocol::MatterPath;
use unicode_normalization::UnicodeNormalization;

use std::cell::Cell;

use super::{
    HeldOpen, fold_name, held_by_code, held_open, locked, owner_in_another_form, owner_names,
};
use crate::test_dir::TestDir;
use crate::write;

fn path(text: &str) -> MatterPath {
    MatterPath::new(text).unwrap()
}

#[test]
fn owner_names_of_a_word_file() {
    assert_eq!(
        owner_names("Contract.docx"),
        ["~$Contract.docx", "~$ontract.docx", "~$ntract.docx"]
    );
    assert_eq!(owner_names("ab"), ["~$ab", "~$b"]);
    assert_eq!(owner_names("a"), ["~$a"]);
    assert_eq!(owner_names("éàb"), ["~$éàb", "~$àb", "~$b"]);
}

/// Every `name_lengths` row of R9's Word for Mac run (private #112,
/// `measurements/g9-versions/results/macos-arm64/word-lock.json`), byte for byte as measured: the
/// owner file Word made is one of the names checked. In the accented row the document's name is
/// NFC and the owner file's NFD (APFS hands back the NFD spelling), so the NFC name checked differs
/// from it in bytes; the match on disk relies on APFS looking names up without regard to
/// normalisation (`nfd_owner_file_found_by_nfc_name`, macOS only). `compose` makes the
/// measured name NFC for the comparison here.
#[test]
fn owner_names_cover_words_rule() {
    let rows = [
        ("a.docx", "~$a.docx"),
        ("ab.docx", "~$ab.docx"),
        ("abc.docx", "~$abc.docx"),
        ("abcd.docx", "~$abcd.docx"),
        ("abcde.docx", "~$abcde.docx"),
        ("abcdef.docx", "~$abcdef.docx"),
        ("abcdefg.docx", "~$bcdefg.docx"),
        ("abcdefgh.docx", "~$cdefgh.docx"),
        ("abcdefghijklmnopqrst.docx", "~$cdefghijklmnopqrst.docx"),
        (
            "Petition to quash the order of the lower court, final draft v3.docx",
            "~$tition to quash the order of the lower court, final draft v3.docx",
        ),
        (
            "P\u{e9}tition \u{e0} casser \u{f1}.docx",
            "~$tition \u{e0} casser \u{f1}.docx",
        ),
    ];
    for (document, owner) in rows {
        let owner = compose(owner);
        assert!(
            owner_names(document).contains(&owner),
            "{document}: {owner} not in {:?}",
            owner_names(document)
        );
    }
}

#[test]
fn files_with_an_owner_file_beside_them_are_locked() {
    let dir = TestDir::new("locks-owner");
    let root = dir.path();
    write::create_dir_all(&root.join("sub")).unwrap();
    for name in [
        "Contract.docx",
        "sub/Brief.docx",
        "Free.docx",
        "Invoice.docx",
        "~$ntract.docx",
        "sub/~$Brief.docx",
        "~$nvoice.docx",
    ] {
        drop(write::create_new(&root.join(name)).unwrap());
    }
    let found = locked(
        root,
        &[
            path("sub/Brief.docx"),
            path("Free.docx"),
            path("Contract.docx"),
            path("Contract.docx"),
            path("Missing.docx"),
            path("Invoice.docx"),
        ],
    );
    assert_eq!(
        found,
        [
            path("Contract.docx"),
            path("Invoice.docx"),
            path("sub/Brief.docx")
        ]
    );
}

/// `held_open` names the owner file it found, the first of `owner_names` that exists, and says
/// nothing for a file with none.
#[test]
fn held_open_names_the_owner_file() {
    let dir = TestDir::new("locks-held-open");
    let root = dir.path();
    write::create_dir_all(&root.join("sub")).unwrap();
    for name in ["sub/Invoice.docx", "sub/~$nvoice.docx", "Free.docx"] {
        drop(write::create_new(&root.join(name)).unwrap());
    }
    assert_eq!(
        held_open(root, &path("sub/Invoice.docx")),
        Some(HeldOpen::OwnerFile("~$nvoice.docx".to_owned()))
    );
    assert_eq!(held_open(root, &path("Free.docx")), None);
    assert_eq!(held_open(root, &path("Invoice.docx")), None);
}

#[test]
fn a_missing_file_with_no_owner_file_is_not_locked() {
    let dir = TestDir::new("locks-missing");
    assert_eq!(locked(dir.path(), &[path("gone/x.docx")]), []);
}

/// `text` with the decomposed letters of word-lock.json's accented row (`e`, `a` and `n` with
/// their combining marks) composed, as NFC writes them. Covers that row only, not NFC at large.
fn compose(text: &str) -> String {
    text.replace("e\u{301}", "\u{e9}")
        .replace("a\u{300}", "\u{e0}")
        .replace("n\u{303}", "\u{f1}")
}

/// macOS: Word's owner file, named in NFD as APFS returned it, is found by the NFC name the
/// check builds from an NFC document name: APFS looks names up without regard to normalisation.
#[cfg(target_os = "macos")]
#[test]
fn nfd_owner_file_found_by_nfc_name() {
    let dir = TestDir::new("locks-nfd");
    let root = dir.path();
    let document = "P\u{e9}tition \u{e0} casser \u{f1}.docx";
    for name in [document, "~$tition a\u{300} casser n\u{303}.docx"] {
        drop(write::create_new(&root.join(name)).unwrap());
    }
    assert_eq!(locked(root, &[path(document)]), [path(document)]);
}

/// `ऱानून.docx` as NFC (U+0931 …) and as NFD (U+0930 U+093C …), in that order. The nukta letter
/// sits in the first characters, so the owner names cut from the two forms differ. (`क़` is not
/// used: U+0958 is a composition exclusion, so its NFC form is the decomposed one.)
fn nukta_names() -> (String, String) {
    let composed = "\u{931}\u{93e}\u{928}\u{942}\u{928}.docx";
    (composed.nfc().collect(), composed.nfd().collect())
}

/// `~$` and the NFD name without its base letter: the owner name Word makes when the cut lands
/// after the base letter, so it starts with a lone combining mark. It is in the decomposed
/// name's owner names and in none of the composed name's, as [`fold_name`] folds them.
fn owner_only_in_decomposed(composed: &str, decomposed: &str) -> String {
    let owner = String::from("~$\u{93c}\u{93e}\u{928}\u{942}\u{928}.docx");
    assert!(owner_names(decomposed).contains(&owner));
    assert!(
        !owner_names(composed)
            .iter()
            .any(|name| fold_name(name) == owner)
    );
    owner
}

/// An Office owner file cut from the decomposed (NFD) name is found for the composed (NFC) name,
/// through the decomposed form that `owner_in_another_form` builds from it. The owner file is
/// given, not on disk (the folder is empty), so the match comes from the names alone.
#[test]
fn an_owner_file_cut_from_the_decomposed_name_is_found_for_the_composed_name() {
    let dir = TestDir::new("locks-decomposed-owner");
    let (composed, decomposed) = nukta_names();
    assert_ne!(composed, decomposed);
    assert_ne!(owner_names(&composed), owner_names(&decomposed));
    let owner = owner_only_in_decomposed(&composed, &decomposed);
    assert_eq!(
        owner_in_another_form(dir.path(), &composed, Some(std::slice::from_ref(&owner))),
        Some(owner)
    );
}

/// The same through the other direction: the decomposed name is the name as written, and the
/// owner file cut from it is found for that name. Its owner names come from the loop over the
/// name as written and its decomposed form, not from the composed name's list.
#[test]
fn an_owner_file_cut_from_the_decomposed_name_is_found_for_the_decomposed_name() {
    let dir = TestDir::new("locks-decomposed-written");
    let (composed, decomposed) = nukta_names();
    let owner = owner_only_in_decomposed(&composed, &decomposed);
    assert_eq!(
        owner_in_another_form(dir.path(), &decomposed, Some(std::slice::from_ref(&owner))),
        Some(owner)
    );
}

/// A Windows rename or delete that failed with a sharing or lock violation (32, 33) is a file
/// held open, without asking whether it is held; an access denied (5) only when the file is held
/// at that moment, since 5 is also a plain permission denial; any other code, or none, never is.
#[test]
fn a_write_failure_is_held_by_its_code_and_access_denied_only_when_held() {
    let asked = Cell::new(0);
    let held = |answer: bool| {
        let asked = &asked;
        move || {
            asked.set(asked.get() + 1);
            answer
        }
    };
    assert!(held_by_code(Some(32), held(false)), "sharing violation");
    assert!(held_by_code(Some(33), held(false)), "lock violation");
    assert_eq!(asked.get(), 0, "32 and 33 need no check");
    assert!(held_by_code(Some(5), held(true)), "access denied, held");
    assert!(
        !held_by_code(Some(5), held(false)),
        "access denied, not held"
    );
    assert_eq!(asked.get(), 2, "5 asks once each time");
    for other in [
        None,
        Some(0),
        Some(2),
        Some(3),
        Some(13),
        Some(112),
        Some(1224),
    ] {
        assert!(!held_by_code(other, held(true)), "{other:?}");
    }
    assert_eq!(asked.get(), 2, "no other code asks");
}

/// Off Windows an open file never stops a rename or delete, so no write failure is a held file,
/// whatever its code (5 is `EIO` on Linux and macOS, 32 `EPIPE`).
#[cfg(not(windows))]
#[test]
fn off_windows_no_write_failure_is_held() {
    for code in [5, 32, 33] {
        let err = std::io::Error::from_raw_os_error(code);
        assert!(!super::held_at_write(&err, || true), "{code}");
    }
}
