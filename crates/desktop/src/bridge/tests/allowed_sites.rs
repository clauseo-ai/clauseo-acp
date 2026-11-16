//! The unsafe-site override (0053, PR 2) goes by window: `AllowBlockedSite` from `main` alone,
//! `RevokeAllowedSite` and the `AllowedSites` read from `main` and `settings`, and every other
//! label (another window, a webview inside one, whatever an agent's output could open) is
//! refused with `command.invalid` before the host sees it.

use clauseo_protocol::{BlockedSiteId, MatterId};
use serde_json::json;

use super::*;

/// Labels that are neither window: another window, and webviews named after one.
const OTHERS: [&str; 3] = ["other", "main-preview", "settings/frame"];

fn command(body: serde_json::Value) -> HostCommand {
    serde_json::from_value(body).unwrap()
}

#[test]
fn allowing_and_revoking_go_by_window() {
    let scratch = Scratch::new("admit-allowed-sites", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let block_id = BlockedSiteId::new(Uuid::now_v7()).unwrap();
    let matter = MatterId::new(Uuid::now_v7()).unwrap();
    let allow = json!({"allow_blocked_site": {"block_id": block_id, "scope": "always"}});
    let revoke = json!({"revoke_allowed_site":
        {"host": "evil.example", "scope": "matter", "matter": matter}});
    let mut rows = vec![
        (MAIN, &allow, true),
        (MAIN, &revoke, true),
        (SETTINGS, &allow, false),
        (SETTINGS, &revoke, true),
    ];
    for other in OTHERS {
        rows.extend([(other, &allow, false), (other, &revoke, false)]);
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
    // Through `dispatch`: a refused window never reaches the host, whose own refusal of an
    // unknown notice names the notice; `main`'s does reach it.
    for window in [SETTINGS, OTHERS[0], OTHERS[1], OTHERS[2]] {
        let envelope = CommandEnvelope {
            command_id: CommandId::new(Uuid::now_v7()).unwrap(),
            command: command(allow.clone()),
        };
        let refusal = block_on(bridge.dispatch(window, envelope)).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window}");
        assert!(
            !refusal.message().contains("notice"),
            "{window}: {refusal:?}"
        );
    }
    let envelope = CommandEnvelope {
        command_id: CommandId::new(Uuid::now_v7()).unwrap(),
        command: command(allow),
    };
    let refusal = block_on(bridge.dispatch(MAIN, envelope)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::BlocklistNoticeExpired);
    assert!(refusal.message().contains("notice"), "{refusal:?}");
}

#[test]
fn the_allowed_sites_are_read_by_main_and_settings_only() {
    let scratch = Scratch::new("query-allowed-sites", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    for window in [MAIN, SETTINGS] {
        let answer = block_on(bridge.query(window, HostQuery::AllowedSites));
        assert_eq!(
            answer,
            Ok(QueryResult::AllowedSites(Vec::new())),
            "{window}"
        );
    }
    for window in OTHERS {
        let refusal = block_on(bridge.query(window, HostQuery::AllowedSites)).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window}");
    }
}
