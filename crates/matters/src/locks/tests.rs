use clauseo_protocol::MatterPath;

use super::{locked, owner_names};
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
