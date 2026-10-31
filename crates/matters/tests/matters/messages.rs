//! What the errors and the settings say when shown: each reason a folder is missing or refused
//! in words, the locked files by name, and the settings without their probe.

use std::path::PathBuf;
use std::sync::Arc;

use clauseo_matters::{Error, MissingReason, Refusal};

use crate::fixture::{matter_id, path};
use crate::support::{self, MarkedProbe};

/// Each reason a matter folder is missing reads as words a person can act on.
#[test]
fn missing_reasons_say_why() {
    let cases = [
        (MissingReason::NoFolder, "it does not exist".to_owned()),
        (
            MissingReason::NoMatterJson,
            "it has no .clauseo/matter.json".to_owned(),
        ),
        (
            MissingReason::Unreadable,
            "its .clauseo/matter.json does not read".to_owned(),
        ),
        (
            MissingReason::OtherMatter(matter_id(7)),
            format!("its .clauseo/matter.json names matter {}", matter_id(7)),
        ),
    ];
    for (reason, words) in cases {
        assert_eq!(reason.to_string(), words);
        let err = Error::FolderMissing {
            folder: PathBuf::from("/matters/Smith"),
            reason,
        };
        assert_eq!(
            err.to_string(),
            format!("the matter folder /matters/Smith is missing: {words}")
        );
    }
}

/// Each reason a folder cannot be a matter reads as words, naming the listed entry or the
/// matter it already belongs to.
#[test]
fn refusals_say_why() {
    let cases = [
        (Refusal::DriveRoot, "it is a drive root".to_owned()),
        (
            Refusal::Listed(PathBuf::from("/home/lawyer")),
            "it is or contains /home/lawyer".to_owned(),
        ),
        (
            Refusal::AlreadyMatter(matter_id(3)),
            format!("it is already matter {}", matter_id(3)),
        ),
        (
            Refusal::UnreadableMatterJson,
            "its .clauseo/matter.json does not read, so it may be a matter".to_owned(),
        ),
        (
            Refusal::MattersRoot,
            "it is, holds or sits inside Clauseo's matters folder".to_owned(),
        ),
    ];
    for (reason, words) in cases {
        assert_eq!(reason.to_string(), words);
        let err = Error::FolderRefused {
            folder: PathBuf::from("/home"),
            reason,
        };
        assert_eq!(
            err.to_string(),
            format!("/home cannot be a matter: {words}")
        );
    }
}

/// A locked-files error names every locked file, in order, separated by commas.
#[test]
fn file_locked_names_every_file() {
    let err = Error::FileLocked {
        paths: vec![path("Brief.docx"), path("exhibits/A.pdf")],
    };
    assert_eq!(
        err.to_string(),
        "files are open in another program: Brief.docx, exhibits/A.pdf"
    );
    let one = Error::FileLocked {
        paths: vec![path("memo.txt")],
    };
    assert_eq!(
        one.to_string(),
        "files are open in another program: memo.txt"
    );
}

/// The settings' debug form shows the matters root, the limits and the retention.
#[test]
fn settings_debug_shows_root_limits_and_retention() {
    let settings = support::settings("/matters-root", Arc::new(MarkedProbe::default()));
    let shown = format!("{settings:?}");
    assert!(shown.starts_with("Settings {"), "{shown}");
    assert!(shown.contains("matters_root: \"/matters-root\""), "{shown}");
    assert!(
        shown.contains(&format!("limits: {:?}", settings.limits)),
        "{shown}"
    );
    assert!(
        shown.contains(&format!("retention: {:?}", settings.retention)),
        "{shown}"
    );
}
