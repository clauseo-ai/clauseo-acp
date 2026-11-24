//! `ListAddons`, the one list of skills and tools, is read by both windows: the main window's
//! Skills and Connectors place and Settings. Split from `admit.rs` for its length.

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
