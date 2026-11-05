//! The download's event and commands on the wire.

use super::{BrowserInstallFailure, BrowserInstallState};
use crate::{Count, HostCommand, HostEvent};

/// A failure code a newer host adds decodes to `Unknown`, keeping the event and its message.
#[test]
fn an_unknown_failure_code_decodes_to_unknown() {
    let json = r#"{"browser_install":{"state":{"failed":{"code":"quota","message":"m"}}}}"#;
    let event: HostEvent = serde_json::from_str(json).unwrap();
    assert_eq!(
        event,
        HostEvent::BrowserInstall {
            state: BrowserInstallState::Failed {
                code: BrowserInstallFailure::Unknown,
                message: "m".to_owned(),
            },
        }
    );
}

/// Every failure code is its `snake_case` name on the wire, the list the app's copy keys on.
#[test]
fn the_failure_codes_are_these_strings() {
    let names: Vec<String> = BrowserInstallFailure::ALL
        .iter()
        .map(|code| serde_json::to_string(code).unwrap())
        .collect();
    assert_eq!(
        names,
        [
            r#""network""#,
            r#""size_mismatch""#,
            r#""disk_space""#,
            r#""host_refused""#,
            r#""signature""#,
            r#""unpack""#,
            r#""unsupported""#,
            r#""cancelled""#,
            r#""unknown""#,
        ]
    );
}

/// The states' JSON, as the app reads them.
#[test]
fn the_states_are_written_as_the_app_reads_them() {
    let json = |state: BrowserInstallState| serde_json::to_string(&state).unwrap();
    assert_eq!(
        json(BrowserInstallState::Downloading {
            bytes: Count::from(5u32),
            total: Count::from(9u32),
        }),
        r#"{"downloading":{"bytes":5,"total":9}}"#
    );
    assert_eq!(json(BrowserInstallState::Unpacking), r#""unpacking""#);
    assert_eq!(json(BrowserInstallState::Done), r#""done""#);
}

/// The two commands take no fields, and refuse any.
#[test]
fn the_commands_take_no_fields() {
    let install: HostCommand = serde_json::from_str(r#"{"install_browser":{}}"#).unwrap();
    assert_eq!(install, HostCommand::InstallBrowser {});
    let cancel: HostCommand = serde_json::from_str(r#"{"cancel_browser_install":{}}"#).unwrap();
    assert_eq!(cancel, HostCommand::CancelBrowserInstall {});
    assert!(serde_json::from_str::<HostCommand>(r#"{"install_browser":{"version":"1"}}"#).is_err());
}
