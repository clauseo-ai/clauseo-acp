//! A matter's kept "Always" rules go by window: `RemoveKeptPermission` and the
//! `KeptPermissions` read from `main` alone; the settings window and every other label (another
//! window, a webview inside one) are refused with `command.invalid` before the host sees them.

use serde_json::json;

use super::*;

/// Labels that are not the main window.
const OTHERS: [&str; 4] = [SETTINGS, "other", "main-preview", "settings/frame"];

#[test]
fn removing_a_kept_permission_is_mains_alone() {
    let scratch = Scratch::new("admit-kept-permissions", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let body = json!({"remove_kept_permission": {"matter": host.scratch(), "rule": "Bash(ls)"}});
    let remove: HostCommand = serde_json::from_value(body).unwrap();
    assert_eq!(bridge.admit(MAIN, &remove), Ok(()));
    for window in OTHERS {
        let refusal = bridge.admit(window, &remove).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window}");
    }
    // `main`'s reaches the host, which removes nothing from a matter that keeps nothing.
    let envelope = CommandEnvelope {
        command_id: CommandId::new(Uuid::now_v7()).unwrap(),
        command: remove,
    };
    assert_eq!(
        block_on(bridge.dispatch(MAIN, envelope)),
        Ok(CommandOutcome::Done)
    );
}

#[test]
fn the_kept_permissions_are_read_by_main_alone() {
    let scratch = Scratch::new("query-kept-permissions", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let query = HostQuery::KeptPermissions {
        matter: host.scratch(),
    };
    let answer = block_on(bridge.query(MAIN, query.clone()));
    assert_eq!(answer, Ok(QueryResult::KeptPermissions(Vec::new())));
    for window in OTHERS {
        let refusal = block_on(bridge.query(window, query.clone())).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{window}");
    }
}
