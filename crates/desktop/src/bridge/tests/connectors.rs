//! The connector commands by window (R24, the app review of #470): `main` shows agent output, so
//! of them it sends only `dismiss_case_law_hint` and `refresh_balance`; adding a connector or a
//! key, buying credits, signing out and removing are the settings window's, as `SetSkillPack`
//! is. The settings window never dismisses the room's hint. Another label sends none. Each
//! refusal is `command.invalid` from the bridge, before the host sees the command.

use clauseo_protocol::MatterId;
use serde_json::{Value, json};

use super::*;

/// Every connector command, as a window writes it, with whether `main` may send it.
fn commands() -> Vec<(Value, bool)> {
    let matter = MatterId::new(Uuid::now_v7()).unwrap();
    vec![
        (json!({"check": {}}), false),
        (json!({"add": {"id": "case-law"}}), false),
        (
            json!({"add_api_key": {"id": "case-law", "key": "pasted-key-for-the-window-test"}}),
            false,
        ),
        (
            json!({"add_credits": {"id": "case-law", "pack": "200"}}),
            false,
        ),
        (json!({"sign_out": {"id": "case-law"}}), false),
        (json!({"remove": {"id": "case-law"}}), false),
        (json!({"refresh_balance": {"id": "case-law"}}), true),
        (json!({"open_pay_link": {"id": "case-law"}}), false),
        (json!({"dismiss_case_law_hint": {"matter": matter}}), true),
    ]
}

fn connector(body: &Value) -> HostCommand {
    let text = body.to_string();
    serde_json::from_value(json!({ "connector": body }))
        .unwrap_or_else(|err| panic!("{text}: {err}"))
}

/// The table, through `admit`: `main` the two, Settings all but the hint, another label none.
#[test]
fn each_connector_command_is_admitted_from_its_window_only() {
    let scratch = Scratch::new("admit-connectors", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    for (body, from_main) in commands() {
        let command = connector(&body);
        let dismiss = body.get("dismiss_case_law_hint").is_some();
        let main = bridge.admit(MAIN, &command);
        assert_eq!(main.is_ok(), from_main, "main: {body}");
        let settings = bridge.admit(SETTINGS, &command);
        assert_eq!(settings.is_ok(), !dismiss, "settings: {body}");
        for refused in [main.err(), settings.err()].into_iter().flatten() {
            assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{body}");
            assert!(!refused.message().contains("pasted-key"), "{refused}");
        }
        let other = bridge.admit("other", &command).unwrap_err();
        assert_eq!(other.code(), ErrorCode::CommandInvalid, "other: {body}");
    }
}

/// Through `dispatch`, as the page calls it: each settings-only command from `main` is refused
/// by the bridge (`command.invalid`, its message naming the rule), not by the host, which has
/// no connectors wired here and would refuse with its own words.
#[test]
fn the_main_window_cannot_dispatch_a_settings_only_connector_command() {
    let scratch = Scratch::new("dispatch-connectors-main", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    for (body, from_main) in commands() {
        if from_main {
            continue;
        }
        let refused = block_on(bridge.dispatch(MAIN, envelope(connector(&body)))).unwrap_err();
        assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{body}");
        assert!(
            refused.message().contains("sent from Settings"),
            "{body}: {refused}"
        );
    }
}
