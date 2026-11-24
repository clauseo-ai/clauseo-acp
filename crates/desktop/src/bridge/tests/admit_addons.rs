//! `ListAddons`, the one list of skills and tools, is read by both windows: the main window's
//! Skills and Connectors place and Settings; `SetAddonScope`, `SetAddonOn` and `RemoveAddon`
//! come from both too. Split from `admit.rs` for its length.

use serde_json::json;

use super::*;

/// A query as the page writes it.
fn read(body: serde_json::Value) -> HostQuery {
    let text = body.to_string();
    serde_json::from_value(body).unwrap_or_else(|err| panic!("{text}: {err}"))
}

/// `list_addons`, with and without a matter, answers from `main` and the settings window, with
/// the built-ins first; any other window is refused with `command.invalid`.
#[test]
fn list_addons_from_main_and_settings_is_admitted() {
    let scratch = Scratch::new("admit-list-addons", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let matter = clauseo_protocol::MatterId::new(Uuid::now_v7()).unwrap();
    for window in [MAIN, SETTINGS] {
        for body in [
            json!({"list_addons": {}}),
            json!({"list_addons": {"matter": matter}}),
        ] {
            let answer = block_on(bridge.query(window, read(body)));
            let Ok(QueryResult::Addons(list)) = &answer else {
                panic!("{window}: {answer:?}");
            };
            let first = list.items.first().map(|item| item.id.as_str());
            assert_eq!(first, Some("legal-drafting"), "{window}: {answer:?}");
        }
    }
    let other = block_on(bridge.query("other", read(json!({"list_addons": {}})))).unwrap_err();
    assert_eq!(other.code(), ErrorCode::CommandInvalid);
}

/// The three add-on commands pass the bridge from `main` and the settings window (the host
/// answers them: an unknown id is `addon.unknown`, not `command.invalid`); any other window
/// is refused.
#[test]
fn the_addon_commands_from_main_and_settings_are_admitted() {
    let scratch = Scratch::new("admit-addon-commands", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let matter = clauseo_protocol::MatterId::new(Uuid::now_v7()).unwrap();
    let bodies = [
        json!({"set_addon_scope": {"kind": "skill", "id": "nobody", "scope": "all"}}),
        json!({"set_addon_on": {"kind": "tool", "id": "nobody", "matter": matter, "on": false}}),
        json!({"remove_addon": {"kind": "tool", "id": "nobody"}}),
    ];
    for window in [MAIN, SETTINGS] {
        for body in &bodies {
            let command: HostCommand = serde_json::from_value(body.clone()).unwrap();
            assert!(bridge.admit(window, &command).is_ok(), "{window}: {body}");
        }
    }
    for body in &bodies {
        let command: HostCommand = serde_json::from_value(body.clone()).unwrap();
        let refused = bridge.admit("other", &command).unwrap_err();
        assert_eq!(refused.code(), ErrorCode::CommandInvalid);
    }
}
