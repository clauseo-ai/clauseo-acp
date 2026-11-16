//! The Python runtime's state on the wire, `SetupInfo`'s optional `python`; see
//! `../setup.rs`. Every field added after the first form is optional both ways.

use clauseo_protocol::{
    Count, ErrorCode, InstallLog, PythonBlockedBy, PythonFailureCause, PythonProgress, SetupEvent,
};

fn hosts() -> Vec<String> {
    vec![
        "github.com".to_owned(),
        "release-assets.githubusercontent.com".to_owned(),
    ]
}

/// Every Python state: each failure cause bare, the download-side ones with their code (a
/// blocked one with hosts), and a refused start with each `blocked_by`.
pub(crate) fn every_python() -> Vec<PythonProgress> {
    let log = InstallLog::new(["Clauseo download log".to_owned()]);
    let mut all = vec![
        PythonProgress::Unavailable,
        PythonProgress::Missing {
            download_bytes: Count::from(70_000_000_u32),
        },
        PythonProgress::Waiting {
            code: None,
            hosts: Vec::new(),
            log: log.clone(),
        },
        PythonProgress::Waiting {
            code: Some(ErrorCode::AgentInstallOffline),
            hosts: hosts(),
            log: log.clone(),
        },
        PythonProgress::Downloading {
            received: Count::from(1_u32),
            total: Count::from(2_u32),
        },
        PythonProgress::Installing,
        PythonProgress::Ready {
            version: "3.13.18".to_owned(),
        },
    ];
    let failed = |cause, code, hosts, blocked_by| PythonProgress::Failed {
        cause,
        code,
        hosts,
        blocked_by,
        log: log.clone(),
    };
    for cause in [
        PythonFailureCause::Blocked,
        PythonFailureCause::DiskFull,
        PythonFailureCause::Failed,
        PythonFailureCause::RunBlocked,
        PythonFailureCause::RunFailed,
    ] {
        all.push(failed(cause, None, Vec::new(), None));
    }
    all.push(failed(
        PythonFailureCause::Blocked,
        Some(ErrorCode::AgentInstallBlocked),
        hosts(),
        None,
    ));
    all.push(failed(
        PythonFailureCause::DiskFull,
        Some(ErrorCode::AgentInstallDiskFull),
        Vec::new(),
        None,
    ));
    for by in [
        PythonBlockedBy::AppControl,
        PythonBlockedBy::AppLocker,
        PythonBlockedBy::Unknown,
    ] {
        all.push(failed(
            PythonFailureCause::RunBlocked,
            None,
            Vec::new(),
            Some(by),
        ));
    }
    all
}

/// An older window's decoder ignores `python`, and an older host's `SetupInfo` (no `python`)
/// decodes with none.
#[test]
fn setup_info_s_python_is_optional_both_ways() {
    let old: SetupEvent =
        serde_json::from_str(r#"{"setup_info":{"keys_available":true}}"#).unwrap();
    assert_eq!(
        old,
        SetupEvent::SetupInfo {
            keys_available: true,
            python: None,
        }
    );
}

/// An older host's `waiting` and `failed` (no `code`, `hosts` or `blocked_by`) decode with
/// none of them.
#[test]
fn python_s_later_fields_are_optional() {
    let waiting: PythonProgress = serde_json::from_str(r#"{"waiting":{"log":["x"]}}"#).unwrap();
    assert_eq!(
        waiting,
        PythonProgress::Waiting {
            code: None,
            hosts: Vec::new(),
            log: InstallLog::new(["x".to_owned()]),
        }
    );
    let failed: PythonProgress =
        serde_json::from_str(r#"{"failed":{"cause":"run_blocked","log":["x"]}}"#).unwrap();
    assert_eq!(
        failed,
        PythonProgress::Failed {
            cause: PythonFailureCause::RunBlocked,
            code: None,
            hosts: Vec::new(),
            blocked_by: None,
            log: InstallLog::new(["x".to_owned()]),
        }
    );
}

#[test]
fn every_python_state_round_trips() {
    for python in every_python() {
        let json = serde_json::to_string(&python).unwrap();
        let back: PythonProgress = serde_json::from_str(&json).unwrap();
        assert_eq!(back, python, "{json}");
    }
}

#[test]
fn python_states_have_their_json_form() {
    let log = || InstallLog::new(["x".to_owned()]);
    let failed = |cause, code, hosts, blocked_by| PythonProgress::Failed {
        cause,
        code,
        hosts,
        blocked_by,
        log: log(),
    };
    let blocked = PythonFailureCause::RunBlocked;
    let cases = [
        (
            PythonProgress::Missing {
                download_bytes: Count::from(70_000_000_u32),
            },
            r#"{"missing":{"download_bytes":70000000}}"#,
        ),
        (
            PythonProgress::Waiting {
                code: Some(ErrorCode::AgentInstallOffline),
                hosts: vec!["github.com".to_owned()],
                log: log(),
            },
            r#"{"waiting":{"code":"agent.install_offline","hosts":["github.com"],"log":["x"]}}"#,
        ),
        (
            failed(
                PythonFailureCause::Blocked,
                Some(ErrorCode::AgentInstallBlocked),
                vec!["github.com".to_owned()],
                None,
            ),
            r#"{"failed":{"cause":"blocked","code":"agent.install_blocked","hosts":["github.com"],"log":["x"]}}"#,
        ),
        (
            failed(blocked, None, Vec::new(), None),
            r#"{"failed":{"cause":"run_blocked","log":["x"]}}"#,
        ),
        (
            failed(blocked, None, Vec::new(), Some(PythonBlockedBy::AppControl)),
            r#"{"failed":{"cause":"run_blocked","blocked_by":"app_control","log":["x"]}}"#,
        ),
        (
            failed(blocked, None, Vec::new(), Some(PythonBlockedBy::AppLocker)),
            r#"{"failed":{"cause":"run_blocked","blocked_by":"applocker","log":["x"]}}"#,
        ),
        (
            failed(blocked, None, Vec::new(), Some(PythonBlockedBy::Unknown)),
            r#"{"failed":{"cause":"run_blocked","blocked_by":"unknown","log":["x"]}}"#,
        ),
    ];
    for (python, json) in cases {
        assert_eq!(serde_json::to_string(&python).unwrap(), json);
        let event = SetupEvent::SetupInfo {
            keys_available: false,
            python: Some(python),
        };
        let wrapped = format!(r#"{{"setup_info":{{"keys_available":false,"python":{json}}}}}"#);
        assert_eq!(serde_json::to_string(&event).unwrap(), wrapped);
    }
}
