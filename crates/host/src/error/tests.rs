use std::path::PathBuf;

use clauseo_protocol::{ErrorCode, HostError, MatterPath, Timestamp};

use super::Error;

fn wire(error: Error) -> HostError {
    HostError::from(error)
}

#[test]
fn codes_map() {
    let invalid = Timestamp::new(i64::MAX).unwrap_err();
    assert_eq!(Error::from(invalid).code(), ErrorCode::CommandInvalid);

    let refused = wire(Error::refused(ErrorCode::ThreadTurnRunning, "a turn runs"));
    assert_eq!(refused.code(), ErrorCode::ThreadTurnRunning);
    assert!(refused.retryable());
    assert_eq!(refused.message(), "a turn runs");

    let locked = wire(Error::from(clauseo_store::Error::Locked));
    assert_eq!(locked.code(), ErrorCode::StoreLocked);
    assert!(!locked.retryable());

    let missing = clauseo_matters::Error::FileLocked { paths: Vec::new() };
    assert_eq!(Error::from(missing).code(), ErrorCode::MatterFileLocked);

    let launch = wire(Error::from(clauseo_process::Error::InvalidSpec(
        "x".to_owned(),
    )));
    assert_eq!(launch.code(), ErrorCode::AgentLaunchFailed);
    assert!(launch.retryable());

    let exited = wire(Error::from(clauseo_acp::Error::Closed));
    assert_eq!(exited.code(), ErrorCode::AgentExited);
    assert!(exited.retryable());

    let reducer = clauseo_reducer::Error::SeqOutOfOrder {
        expected: 2,
        got: 5,
    };
    assert_eq!(wire(Error::from(reducer)).code(), ErrorCode::Internal);

    let settings = clauseo_settings::Error::RelativeDataDir {
        path: PathBuf::from("rel"),
    };
    assert_eq!(wire(Error::from(settings)).code(), ErrorCode::Internal);
    let locked = wire(Error::from(clauseo_settings::Error::Locked {
        key: "ui.theme".to_owned(),
    }));
    assert_eq!(locked.code(), ErrorCode::SettingsLocked);
    assert!(!locked.retryable());
    assert_eq!(
        locked.message(),
        "settings: ui.theme is set by policy and cannot be changed"
    );

    let closed = wire(Error::Closed);
    assert_eq!(closed.code(), ErrorCode::Internal);
    assert!(!closed.retryable());
    assert_eq!(
        wire(Error::Internal("bug".to_owned())).code(),
        ErrorCode::Internal
    );
    let canceled = futures::channel::oneshot::Canceled;
    assert!(matches!(Error::from(canceled), Error::Closed));
}

/// A matter's `FileLocked` reaches the wire with its files in `HostError::files` and only
/// their count in the message, taken (`From`) or borrowed (`to_wire`, for a command's other
/// waiters); a refusal keeps what it carries, and other failures name no file.
#[test]
fn file_locked_names_its_files_and_no_path_in_the_message() {
    let paths = vec![
        MatterPath::new("Brief.docx").unwrap(),
        MatterPath::new("sub/Memo.docx").unwrap(),
    ];
    let locked = || {
        Error::from(clauseo_matters::Error::FileLocked {
            paths: paths.clone(),
        })
    };
    for error in [wire(locked()), locked().to_wire()] {
        assert_eq!(error.code(), ErrorCode::MatterFileLocked);
        assert!(error.retryable());
        assert_eq!(error.files(), paths.as_slice());
        assert_eq!(error.message(), "matter: 2 files open in another program");
    }
    let one = Error::from(clauseo_matters::Error::FileLocked {
        paths: vec![MatterPath::new("Brief.docx").unwrap()],
    });
    assert_eq!(
        one.to_wire().message(),
        "matter: 1 file open in another program"
    );
    let refusal = HostError::new(ErrorCode::MatterFileLocked, "x").with_files(paths.clone());
    assert_eq!(Error::Refused(refusal.clone()).to_wire(), refusal);
    assert_eq!(wire(Error::Closed).files(), []);
}

/// A matter's `FileFolderClash` names the entry in the way, then the file restored, in `files`,
/// for the app to name the folder or file to rename and to tell which of the two it is.
#[test]
fn a_file_folder_clash_names_the_entry_in_the_way() {
    let clash = || {
        Error::from(clauseo_matters::Error::FileFolderClash {
            path: MatterPath::new("Drafts/Notice.docx").unwrap(),
            in_way: MatterPath::new("Drafts").unwrap(),
        })
    };
    for error in [wire(clash()), clash().to_wire()] {
        assert_eq!(error.code(), ErrorCode::MatterFileFolderClash);
        assert_eq!(
            error.files(),
            [
                MatterPath::new("Drafts").unwrap(),
                MatterPath::new("Drafts/Notice.docx").unwrap()
            ]
        );
    }
}

/// A matter's `FilesRefused` reaches the wire as `matter.file_refused` with its paths in
/// `HostError::files` (sorted, the named one first), in `HostError::refusal` the cause they share
/// (none when they differ, so no file's cause is given for another's), how many were refused
/// and the operation's count, and counts only in the message and the detail.
#[test]
fn files_refused_names_its_files_and_cause_and_no_path_in_the_message() {
    use clauseo_matters::RefusedBecause as Because;
    let brief = MatterPath::new("Brief.docx").unwrap();
    let memo = MatterPath::new("sub/Memo.docx").unwrap();
    let refused = |second| {
        Error::from(clauseo_matters::Error::FilesRefused {
            files: vec![(brief.clone(), Because::SecondName), (memo.clone(), second)],
            of: 3,
        })
    };
    for error in [
        wire(refused(Because::Permissions)),
        refused(Because::Permissions).to_wire(),
    ] {
        assert_eq!(error.code(), ErrorCode::MatterFileRefused);
        assert!(error.retryable());
        assert_eq!(error.files(), [brief.clone(), memo.clone()].as_slice());
        assert_eq!(error.message(), "matter: 2 of 3 files refused");
        assert_eq!(
            error.refusal(),
            Some(clauseo_protocol::FileRefusal {
                because: None,
                refused: 2,
                of: 3,
            })
        );
    }
    let shared = refused(Because::SecondName).to_wire().refusal();
    assert_eq!(
        shared.and_then(|r| r.because),
        Some(clauseo_protocol::RefusedBecause::SecondName)
    );
    assert_eq!(
        refused(Because::Permissions).detail(),
        "matter: 2 of 3 files refused"
    );
    let causes = [
        (
            Because::Permissions,
            clauseo_protocol::RefusedBecause::Permissions,
        ),
        (
            Because::SecondName,
            clauseo_protocol::RefusedBecause::SecondName,
        ),
        (
            Because::SavedCopyUnconfirmed,
            clauseo_protocol::RefusedBecause::SavedCopyUnconfirmed,
        ),
    ];
    for (because, wired) in causes {
        let only = Error::from(clauseo_matters::Error::FilesRefused {
            files: vec![(brief.clone(), because)],
            of: 1,
        })
        .to_wire();
        assert_eq!(only.message(), "matter: 1 file refused");
        assert_eq!(
            only.refusal().map(|r| (r.because, r.refused, r.of)),
            Some((Some(wired), 1, 1))
        );
        assert!(!only.message().contains("Brief"), "{}", only.message());
    }
}

/// Many refused files: the wire names the first `MAX_ERROR_FILES` of them, and `refusal`
/// counts them all.
#[test]
fn files_refused_names_at_most_the_cap_and_counts_them_all() {
    let files: Vec<_> = (0..clauseo_protocol::MAX_ERROR_FILES + 5)
        .map(|n| {
            let path = MatterPath::new(&format!("f{n:03}.docx")).unwrap();
            (path, clauseo_matters::RefusedBecause::Permissions)
        })
        .collect();
    let count = files.len();
    let error = Error::from(clauseo_matters::Error::FilesRefused {
        files: files.clone(),
        of: count + 1,
    })
    .to_wire();
    assert_eq!(error.files().len(), clauseo_protocol::MAX_ERROR_FILES);
    assert_eq!(error.files().first(), files.first().map(|(path, _)| path));
    let refusal = error.refusal().unwrap();
    assert_eq!(
        (refusal.refused, refusal.of),
        (
            u32::try_from(count).unwrap(),
            u32::try_from(count + 1).unwrap()
        )
    );
}

/// A write in place that failed part way reaches the wire as `matter.file_write_failed`,
/// naming the file in `HostError::files`, whether it was put back and the operation's count in
/// `HostError::write_failure`, and no path in the message or the detail.
#[test]
fn a_partly_written_file_is_named_with_whether_it_was_put_back() {
    let lease = MatterPath::new("Leases/Lease.docx").unwrap();
    for put_back in [true, false] {
        let error = || {
            Error::from(clauseo_matters::Error::PartlyWritten {
                path: lease.clone(),
                put_back,
                of: 3,
                source: std::io::Error::other("disk full"),
            })
        };
        let wired = wire(error());
        assert_eq!(wired.code(), ErrorCode::MatterFileWriteFailed);
        assert!(wired.retryable());
        assert_eq!(wired.files(), [lease.clone()].as_slice());
        assert_eq!(
            wired.write_failure(),
            Some(clauseo_protocol::WriteFailure { put_back, of: 3 })
        );
        assert_eq!(wired.refusal(), None);
        for text in [wired.message().to_owned(), error().detail()] {
            assert!(!text.contains("Lease"), "{text}");
        }
    }
}

/// A matter's `FolderMissing` reaches a failure's detail with its reason, never the folder's
/// path, and the wire with its code; a gone folder is `folder_missing`, one without its marker
/// `folder_not_a_matter`.
#[test]
fn a_missing_folder_names_its_reason_and_no_path() {
    let folder = PathBuf::from("/Users/someone/Matters/Acme");
    let cases = [
        (
            clauseo_matters::MissingReason::NoFolder,
            ErrorCode::MatterFolderMissing,
        ),
        (
            clauseo_matters::MissingReason::NoMatterJson,
            ErrorCode::MatterFolderNotAMatter,
        ),
    ];
    for (reason, code) in cases {
        let error = Error::from(clauseo_matters::Error::FolderMissing {
            folder: folder.clone(),
            reason,
        });
        let detail = error.detail();
        let wired = wire(error);
        assert_eq!(wired.code(), code);
        assert!(!detail.contains("someone"), "{detail}");
        assert!(detail.contains(&reason.to_string()), "{detail}");
    }
}
