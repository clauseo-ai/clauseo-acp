//! The policy re-read's event and command on the wire.

use super::BrowserPolicyState;
use crate::{HostCommand, HostEvent};

/// Each state is its `snake_case` name, inside the event as the app reads it.
#[test]
fn the_states_are_written_as_the_app_reads_them() {
    let json = |state| serde_json::to_string(&HostEvent::BrowserPolicy { state }).unwrap();
    assert_eq!(
        json(BrowserPolicyState::Checking),
        r#"{"browser_policy":{"state":"checking"}}"#
    );
    assert_eq!(
        json(BrowserPolicyState::Unreadable),
        r#"{"browser_policy":{"state":"unreadable"}}"#
    );
    assert_eq!(
        json(BrowserPolicyState::Blocked),
        r#"{"browser_policy":{"state":"blocked"}}"#
    );
    assert_eq!(
        json(BrowserPolicyState::Clear),
        r#"{"browser_policy":{"state":"clear"}}"#
    );
}

/// The state is closed: a value this version does not know fails to decode.
#[test]
fn an_unknown_state_is_refused() {
    let json = r#"{"browser_policy":{"state":"maybe"}}"#;
    assert!(serde_json::from_str::<HostEvent>(json).is_err());
}

/// The command takes no fields, and refuses any.
#[test]
fn the_command_takes_no_fields() {
    let recheck: HostCommand = serde_json::from_str(r#"{"recheck_browser_policy":{}}"#).unwrap();
    assert_eq!(recheck, HostCommand::RecheckBrowserPolicy {});
    assert_eq!(
        serde_json::to_string(&recheck).unwrap(),
        r#"{"recheck_browser_policy":{}}"#
    );
    let extra = r#"{"recheck_browser_policy":{"thread":"x"}}"#;
    assert!(serde_json::from_str::<HostCommand>(extra).is_err());
}
