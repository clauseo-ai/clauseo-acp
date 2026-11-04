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
