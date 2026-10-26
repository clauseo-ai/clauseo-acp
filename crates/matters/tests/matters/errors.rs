//! Every `Error` variant and its protocol code, built by hand and, where the API can, for real.

use std::io;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use clauseo_matters::{Error, Matter, MissingReason, NewMatter, Refusal};
use clauseo_protocol::{ErrorCode, FileChoice, FileDecision};

use crate::fixture::{Fixture, cid, matter_id, meta, path, tools, undo, undo_request};

/// Names the variant, with no wildcard, so a new variant fails to compile here until it is
/// added to the list below.
fn variant(err: &Error) -> &'static str {
    match err {
        Error::FolderMissing { .. } => "FolderMissing",
        Error::FolderRefused { .. } => "FolderRefused",
        Error::FileLocked { .. } => "FileLocked",
        Error::UnknownCheckpoint { .. } => "UnknownCheckpoint",
        Error::MissingDecision { .. } => "MissingDecision",
        Error::UnexpectedDecision { .. } => "UnexpectedDecision",
        Error::FolderExists { .. } => "FolderExists",
        Error::HistoryExists { .. } => "HistoryExists",
        Error::MarkerNotBuilt => "MarkerNotBuilt",
        Error::Io { .. } => "Io",
        Error::History { .. } => "History",
    }
}

fn assert_code(err: &Error, code: ErrorCode, name: &str) {
    assert_eq!(variant(err), name, "{err:?}");
    assert_eq!(err.code(), code, "{err:?}");
}

/// One of each variant, with the code it maps to.
fn hand_built() -> [(Error, ErrorCode); 11] {
    let folder = PathBuf::from("/m");
    [
        (
            Error::FolderMissing {
                folder: folder.clone(),
                reason: MissingReason::NoMatterJson,
            },
            ErrorCode::MatterFolderMissing,
        ),
        (
            Error::FolderRefused {
                folder: folder.clone(),
                reason: Refusal::DriveRoot,
            },
            ErrorCode::MatterFolderRefused,
        ),
        (
            Error::FileLocked {
                paths: vec![path("a.docx")],
            },
            ErrorCode::MatterFileLocked,
        ),
        (
            Error::UnknownCheckpoint { id: cid(9) },
            ErrorCode::CommandInvalid,
        ),
        (
            Error::MissingDecision { path: path("a") },
            ErrorCode::CommandInvalid,
        ),
        (
            Error::UnexpectedDecision { path: path("a") },
            ErrorCode::CommandInvalid,
        ),
        (
            Error::FolderExists {
                folder: folder.clone(),
            },
            ErrorCode::CommandInvalid,
        ),
        (
            Error::HistoryExists {
                git_dir: folder.clone(),
            },
            ErrorCode::CommandInvalid,
        ),
        (Error::MarkerNotBuilt, ErrorCode::CommandInvalid),
        (
            Error::Io {
                op: "read",
                path: folder,
                source: io::Error::other("disk"),
            },
            ErrorCode::MatterCheckpointFailed,
        ),
        (
            Error::History {
                op: "commit",
                source: "broken".into(),
            },
            ErrorCode::MatterCheckpointFailed,
        ),
    ]
}

#[test]
fn every_error_maps_to_a_code() {
    let all = hand_built();
    let names: std::collections::BTreeSet<&str> = all.iter().map(|(err, _)| variant(err)).collect();
    assert_eq!(names.len(), all.len(), "each variant once");
    for (err, code) in &all {
        assert_eq!(err.code(), *code, "{err:?}");
        assert_ne!(err.to_string(), "");
    }

    // For real, through the API.
    let fx = Fixture::new("errors-real");
    let matter = undo_errors(&fx);
    folder_errors(&fx);
    // A timeline ref that names no commit.
    drop(matter);
    let timeline = fx.git_dir().join("refs/clauseo/timeline");
    crate::support::write(&timeline, "0000000000000000000000000000000000000bad\n");
    let err = fx.open(&fx.folder).unwrap_err();
    assert_code(&err, ErrorCode::MatterCheckpointFailed, "History");
}

/// Unknown checkpoints, bad decisions and a locked file, on matter 1 with step 1..3.
fn undo_errors(fx: &Fixture) -> Matter {
    fx.write("a.txt", "one");
    let mut matter = fx.import();
    matter.before_turn(&meta(2)).unwrap();
    fx.write("a.txt", "two");
    matter.after_turn(&meta(3), &tools(&["a.txt"])).unwrap();

    let err = matter.edited_since(cid(99), cid(3)).unwrap_err();
    assert_code(&err, ErrorCode::CommandInvalid, "UnknownCheckpoint");
    let err = undo(&mut matter, (1, 99), &["a.txt"], (4, 5)).unwrap_err();
    assert_code(&err, ErrorCode::CommandInvalid, "UnknownCheckpoint");
    let err = undo(&mut matter, (1, 3), &[], (4, 5)).unwrap_err();
    assert_code(&err, ErrorCode::CommandInvalid, "MissingDecision");
    let err = undo(&mut matter, (1, 3), &["a.txt", "b.txt"], (4, 5)).unwrap_err();
    assert_code(&err, ErrorCode::CommandInvalid, "UnexpectedDecision");
    let twice = [
        FileDecision {
            path: path("a.txt"),
            choice: FileChoice::Undo,
        },
        FileDecision {
            path: path("a.txt"),
            choice: FileChoice::Keep,
        },
    ];
    let err = matter.undo(&undo_request(1, 3, &twice, 4, 5)).unwrap_err();
    assert_code(&err, ErrorCode::CommandInvalid, "UnexpectedDecision");
    fx.write("~$a.txt", "open");
    let err = undo(&mut matter, (1, 3), &["a.txt"], (4, 5)).unwrap_err();
    assert_code(&err, ErrorCode::MatterFileLocked, "FileLocked");
    assert_eq!(matter.timeline().len(), 2);
    matter
}

/// Existing folders and ids, a refused folder, missing matters, the marker, a read failure.
fn folder_errors(fx: &Fixture) {
    let err = Matter::create(
        fx.settings(),
        NewMatter {
            id: matter_id(2),
            ..fx.new_matter()
        },
    )
    .unwrap_err();
    assert_code(&err, ErrorCode::CommandInvalid, "FolderExists");

    // The id is in use: importing another folder as matter 1.
    let cancel = AtomicBool::new(false);
    let elsewhere = fx.dir.path().join("elsewhere");
    crate::support::create_dir_all(&elsewhere);
    let new = NewMatter {
        folder: elsewhere.clone(),
        ..fx.new_matter()
    };
    let err = Matter::import(fx.settings(), new, &[], &mut |_| {}, &cancel).unwrap_err();
    assert_code(&err, ErrorCode::CommandInvalid, "HistoryExists");
    assert!(!elsewhere.join(".clauseo").exists());

    // A listed folder.
    let new = NewMatter {
        id: matter_id(3),
        folder: elsewhere.clone(),
        ..fx.new_matter()
    };
    let refused = [elsewhere.clone()];
    let err = Matter::import(fx.settings(), new, &refused, &mut |_| {}, &cancel).unwrap_err();
    assert_code(&err, ErrorCode::MatterFolderRefused, "FolderRefused");
    assert!(!fx.root().join(matter_id(3).to_string()).exists());

    let err = Matter::open(fx.settings(), matter_id(1), &elsewhere).unwrap_err();
    assert_code(&err, ErrorCode::MatterFolderMissing, "FolderMissing");
    assert!(matches!(
        err,
        Error::FolderMissing {
            reason: MissingReason::NoMatterJson,
            ..
        }
    ));
    crate::support::write(elsewhere.join(".clauseo/matter.json"), "not json");
    let err = Matter::open(fx.settings(), matter_id(1), &elsewhere).unwrap_err();
    assert!(matches!(
        err,
        Error::FolderMissing {
            reason: MissingReason::Unreadable,
            ..
        }
    ));

    let err = Matter::create(
        fx.settings(),
        NewMatter {
            marker: clauseo_matters::Marker::EmptyRepository,
            ..fx.new_matter()
        },
    )
    .unwrap_err();
    assert_code(&err, ErrorCode::CommandInvalid, "MarkerNotBuilt");

    // A file that cannot be read fails the import, which leaves nothing behind.
    #[cfg(unix)]
    {
        let unreadable = fx.dir.path().join("unreadable");
        crate::support::write(unreadable.join("secret.txt"), "no access");
        crate::support::set_mode(unreadable.join("secret.txt"), 0o000);
        let new = NewMatter {
            id: matter_id(4),
            folder: unreadable.clone(),
            ..fx.new_matter()
        };
        let err = Matter::import(fx.settings(), new, &[], &mut |_| {}, &cancel).unwrap_err();
        assert_code(&err, ErrorCode::MatterCheckpointFailed, "Io");
        assert!(!fx.root().join(matter_id(4).to_string()).exists());
        assert!(!unreadable.join(".clauseo").exists());
    }
}
