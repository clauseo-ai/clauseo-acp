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
/// `measurements/g9-versions/results/macos-arm64/word-lock.json`): the owner file Word made is
/// one of the names checked. The accented row is written here in NFC, as the document was.
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
        assert!(
            owner_names(document).iter().any(|name| name == owner),
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
