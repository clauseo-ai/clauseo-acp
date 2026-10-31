use std::path::PathBuf;

use clauseo_protocol::{ErrorCode, HostError, Timestamp};

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
