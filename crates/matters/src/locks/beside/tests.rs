use clauseo_protocol::MatterPath;

use super::{OwnersBeside, loose};
use crate::locks::held_open_beside;
use crate::test_dir::TestDir;
use crate::write;

/// `ऱिपोर्ट.docx` composed and decomposed; Word drops one character of its 7-character stem.
const REPORT_NFC: &str = "\u{931}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}.docx";
const REPORT_NFD: &str = "\u{930}\u{93c}\u{93f}\u{92a}\u{94b}\u{930}\u{94d}\u{91f}.docx";
/// `क़ानून.docx`, precomposed and decomposed nukta.
const LAW_PRECOMPOSED: &str = "\u{958}\u{93e}\u{928}\u{942}\u{928}.docx";
const LAW_DECOMPOSED: &str = "\u{915}\u{93c}\u{93e}\u{928}\u{942}\u{928}.docx";

/// Documents, and owner files that may sit beside them: Word's rule, another case, another
/// Unicode form, letters whose case or form mappings meet other letters.
const DOCUMENTS: &[&str] = &[
    "Contract.docx",
    "contract.docx",
    "ab",
    "a",
    "Stra\u{df}e.docx",
    "\u{3a3}\u{3bf}\u{3c6}\u{3af}\u{3b1}.docx",
    "\u{212a}ey.docx",
    "\u{17f}ample.docx",
    REPORT_NFC,
    REPORT_NFD,
    LAW_PRECOMPOSED,
    LAW_DECOMPOSED,
    "\u{905}\u{928}\u{941}\u{92c}\u{902}\u{927} 42.docx",
];

fn owners_of(document: &str) -> Vec<String> {
    let mut owners = Vec::new();
    for form in [
        document.to_owned(),
        document.to_uppercase(),
        document.to_lowercase(),
        unicode_normalization::UnicodeNormalization::nfc(document).collect(),
        unicode_normalization::UnicodeNormalization::nfd(document).collect(),
    ] {
        for dropped in 0..3 {
            let rest: String = form.chars().skip(dropped).collect();
            if !rest.is_empty() {
                owners.push(format!("~${rest}"));
            }
        }
    }
    owners.sort();
    owners.dedup();
    owners
}

/// The safety property: whenever the owner-file lookup finds an owner file for a document, on
/// this OS's file system, `may_hold` keeps the document. Each pair is made on disk, the owner
/// file alone beside the document.
#[test]
fn every_document_the_lookup_finds_held_is_kept() {
    let dir = TestDir::new("locks-beside-superset");
    let mut pairs = 0;
    for (d, document) in DOCUMENTS.iter().enumerate() {
        let mut owners: Vec<String> = DOCUMENTS
            .iter()
            .flat_map(|other| owners_of(other))
            .collect();
        owners.sort();
        owners.dedup();
        for (o, owner) in owners.iter().enumerate() {
            let folder = dir.path().join(format!("{d}-{o}"));
            write::create_dir_all(&folder).unwrap();
            drop(write::create_new(&folder.join(document)).unwrap());
            // Another case of the same name is the same file on macOS and Windows.
            if write::create_new(&folder.join(owner)).is_err() {
                continue;
            }
            let entries = vec![owner.clone()];
            let path = MatterPath::new(document).unwrap();
            let found = held_open_beside(&folder, &path, &entries).is_some();
            let kept = OwnersBeside::new(&entries).may_hold(document);
            assert!(
                !found || kept,
                "{document:?} beside {owner:?}: found but not kept"
            );
            pairs += usize::from(found);
        }
    }
    // Word's own owner names at least, for every document.
    assert!(pairs >= DOCUMENTS.len(), "{pairs}");
}

/// Word's owner names, in any case or Unicode form, keep the document on every OS (the loose
/// match is the same everywhere; only the probe after it differs).
#[test]
fn owner_names_in_any_case_or_form_keep_the_document() {
    for document in DOCUMENTS {
        for owner in owners_of(document) {
            assert!(
                OwnersBeside::new(std::slice::from_ref(&owner)).may_hold(document),
                "{document:?} beside {owner:?}"
            );
        }
    }
}

#[test]
fn a_name_no_owner_file_points_at_is_not_kept() {
    let contract = OwnersBeside::new(&["~$ntract.docx".to_owned(), "notes.txt".to_owned()]);
    for name in [
        "Free.docx",
        "\u{905}\u{928}\u{941}\u{92c}\u{902}\u{927} 42.docx",
        "Contract.pdf",
        "ntract.docx.bak",
    ] {
        assert!(!contract.may_hold(name), "{name:?}");
    }
    assert!(contract.may_hold("Contract.docx"));
    assert!(contract.may_hold("Xontract.docx"));
    // No `~$` entry: nothing is kept.
    assert!(!OwnersBeside::new(&["Contract.docx".to_owned()]).may_hold("Contract.docx"));
    assert!(!OwnersBeside::new(&[]).may_hold("Contract.docx"));
}

#[test]
fn loose_forms_meet_across_case_and_unicode_form() {
    for (a, b) in [
        ("Stra\u{df}e", "STRASSE"),
        ("STRA\u{1e9e}E", "strasse"),
        ("\u{3a3}\u{3c3}\u{3c2}", "\u{3c3}\u{3c3}\u{3c3}"),
        ("\u{17f}", "S"),
        ("\u{212a}", "k"),
        ("\u{212b}", "\u{e5}"),
        ("e\u{301}", "\u{c9}"),
        (REPORT_NFC, REPORT_NFD),
        (LAW_PRECOMPOSED, LAW_DECOMPOSED),
    ] {
        assert_eq!(loose(a), loose(b), "{a:?} {b:?}");
    }
    assert_ne!(loose("Contract"), loose("Kontract"));
}
