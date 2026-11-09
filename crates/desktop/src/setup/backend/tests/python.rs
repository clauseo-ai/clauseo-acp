//! The Python runtime's state on the wire (`entry::python_progress`): a waiting or
//! download-blocked install carries Python's hosts and the agent install's code, as an agent's
//! does; a refused start carries what refused it, and no hosts.

use clauseo_agents::testing::python_failed;
use clauseo_agents::{FailureKind, InstallLog, PythonBlocker, PythonFailureKind, PythonState};
use clauseo_protocol::{ErrorCode, PythonBlockedBy, PythonFailureCause, PythonProgress};

use super::claude;
use crate::setup::backend::entry::python_progress;

fn hosts() -> Vec<String> {
    vec![
        "github.com".to_owned(),
        "release-assets.githubusercontent.com".to_owned(),
    ]
}

/// The wire's `(cause, code, hosts, blocked_by)` of a failed `state`.
fn failed_parts(
    state: &PythonState,
) -> (
    PythonFailureCause,
    Option<ErrorCode>,
    Vec<String>,
    Option<PythonBlockedBy>,
) {
    match python_progress(state, &hosts()) {
        Some(PythonProgress::Failed {
            cause,
            code,
            hosts,
            blocked_by,
            ..
        }) => (cause, code, hosts, blocked_by),
        other => panic!("not failed: {other:?}"),
    }
}

#[test]
fn a_waiting_python_carries_its_hosts_and_the_offline_code() {
    let state = PythonState::Waiting {
        reason: FailureKind::Offline,
        log: InstallLog::new(&claude()),
    };
    let Some(PythonProgress::Waiting {
        code, hosts: got, ..
    }) = python_progress(&state, &hosts())
    else {
        panic!("not waiting");
    };
    assert_eq!(
        code,
        Some(ErrorCode::from_str_lossy(FailureKind::Offline.code()))
    );
    assert_eq!(
        code.map(|code| code.as_str()),
        Some("agent.install_offline")
    );
    assert_eq!(got, hosts());
}

#[test]
fn a_blocked_python_download_carries_its_hosts_and_the_blocked_code() {
    let state = python_failed(&claude(), PythonFailureKind::Blocked, None);
    let (cause, code, got, by) = failed_parts(&state);
    assert_eq!(cause, PythonFailureCause::Blocked);
    assert_eq!(
        code.map(|code| code.as_str()),
        Some("agent.install_blocked")
    );
    assert_eq!(got, hosts());
    assert_eq!(by, None);
}

#[test]
fn other_python_failures_carry_no_hosts() {
    let cases = [
        (
            PythonFailureKind::DiskFull,
            PythonFailureCause::DiskFull,
            Some("agent.install_disk_full"),
        ),
        (
            PythonFailureKind::Failed,
            PythonFailureCause::Failed,
            Some("agent.install_failed"),
        ),
        (
            PythonFailureKind::RunFailed,
            PythonFailureCause::RunFailed,
            None,
        ),
    ];
    for (kind, want, want_code) in cases {
        let (cause, code, got, by) = failed_parts(&python_failed(&claude(), kind, None));
        assert_eq!(cause, want);
        assert_eq!(code.map(|code| code.as_str()), want_code);
        assert_eq!(got, Vec::<String>::new(), "{kind:?}");
        assert_eq!(by, None);
    }
}

#[test]
fn a_refused_python_names_what_refused_it_and_carries_no_code_or_hosts() {
    let cases = [
        (PythonBlocker::AppControl, PythonBlockedBy::AppControl),
        (PythonBlocker::AppLocker, PythonBlockedBy::AppLocker),
        (PythonBlocker::Unknown, PythonBlockedBy::Unknown),
    ];
    for (blocker, want) in cases {
        let state = python_failed(&claude(), PythonFailureKind::RunBlocked, Some(blocker));
        let (cause, code, got, by) = failed_parts(&state);
        assert_eq!(cause, PythonFailureCause::RunBlocked);
        assert_eq!(code, None);
        assert_eq!(got, Vec::<String>::new());
        assert_eq!(by, Some(want));
    }
}
