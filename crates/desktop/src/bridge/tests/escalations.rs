//! R18's escalations go by window: `RequestEscalation`, `SwitchProtectionOn` and
//! `DismissDenial` from `main` alone, `ForgetAllowance` and the `ListAllowances` read from
//! `main` and `settings`, and every other label (another window, a webview inside one,
//! whatever an agent's output could open) is refused with `command.invalid` before the host
//! sees it. The host does not run the four yet (PR 3): `main`'s reach it and are refused there.

use clauseo_protocol::{AllowanceId, DenialId, MatterId};
use serde_json::json;

use super::*;

/// Labels that are neither window: another window, and webviews named after one.
const OTHERS: [&str; 3] = ["other", "main-preview", "settings/frame"];

fn command(body: serde_json::Value) -> HostCommand {
    serde_json::from_value(body).unwrap()
}

#[test]
fn the_four_commands_go_by_window() {
    let scratch = Scratch::new("admit-escalations", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let denial = DenialId::new(Uuid::now_v7()).unwrap();
    let matter = MatterId::new(Uuid::now_v7()).unwrap();
    let other = MatterId::new(Uuid::now_v7()).unwrap();
    let allowance = AllowanceId::new(Uuid::now_v7()).unwrap();
    let card = json!({"request_escalation": {"request": {"denial": denial}}});
    let off = json!({"request_escalation":
        {"request": {"switch_protection_off": {"matter": matter}}}});
    let files = json!({"request_escalation":
        {"request": {"files_from_other_matter": {"matter": matter, "from": other}}}});
    let on = json!({"switch_protection_on": {"matter": matter}});
    let forget = json!({"forget_allowance": {"matter": matter, "allowance": allowance}});
    let dismiss = json!({"dismiss_denial": {"denial": denial}});
    let main_only = [&card, &off, &files, &on, &dismiss];
    let mut rows = vec![(MAIN, &forget, true), (SETTINGS, &forget, true)];
    for body in main_only {
        rows.extend([(MAIN, body, true), (SETTINGS, body, false)]);
    }
    for label in OTHERS {
        rows.push((label, &forget, false));
        rows.extend(main_only.map(|body| (label, body, false)));
    }
    for (window, body, admitted) in rows {
        match bridge.admit(window, &command(body.clone())) {
            Ok(()) => assert!(admitted, "{window} {body} was admitted"),
            Err(refusal) => {
                assert!(!admitted, "{window} {body}: {refusal:?}");
                assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window} {body}");
            }
        }
    }
    // Through `dispatch`: a refused window never reaches the host, whose own refusal says
    // "not built yet"; `main`'s does reach it.
    for window in [SETTINGS, OTHERS[0], OTHERS[1], OTHERS[2]] {
        let envelope = CommandEnvelope {
            command_id: CommandId::new(Uuid::now_v7()).unwrap(),
            command: command(off.clone()),
        };
        let refusal = block_on(bridge.dispatch(window, envelope)).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window}");
        assert!(
            !refusal.message().contains("not built"),
            "{window}: {refusal:?}"
        );
    }
    for body in [card, off, files, on, forget, dismiss] {
        let envelope = CommandEnvelope {
            command_id: CommandId::new(Uuid::now_v7()).unwrap(),
            command: command(body.clone()),
        };
        let refusal = block_on(bridge.dispatch(MAIN, envelope)).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{body}");
        assert!(refusal.message().contains("not built"), "{refusal:?}");
    }
}

#[test]
fn the_allowances_are_read_by_main_and_settings_only() {
    let scratch = Scratch::new("query-allowances", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    for window in [MAIN, SETTINGS] {
        let answer = block_on(bridge.query(window, HostQuery::ListAllowances));
        assert_eq!(answer, Ok(QueryResult::Allowances(Vec::new())), "{window}");
    }
    for window in OTHERS {
        let refusal = block_on(bridge.query(window, HostQuery::ListAllowances)).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window}");
    }
}
