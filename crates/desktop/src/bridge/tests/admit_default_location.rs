//! `SetDefaultLocation` (the app's move of the matters folder) is main's alone, and names only a
//! folder ref this run's `pick_folder` issued, as `CreateMatter`'s `ExistingFolder` does. Split
//! from `admit.rs` for its length.

use serde_json::json;

use super::*;

/// A command as the page writes it.
fn command(body: serde_json::Value) -> HostCommand {
    let text = body.to_string();
    serde_json::from_value(body).unwrap_or_else(|err| panic!("{text}: {err}"))
}

#[test]
fn set_default_location_is_main_s_and_needs_a_picked_folder() {
    let scratch = Scratch::new("admit-default-location", &["work", "other"]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let work = scratch.path().join("work");
    let picked = block_on(bridge.pick_folder(&Picks(Some(work)), Some(scratch.path()))).unwrap();
    let folder = picked.unwrap().folder;
    let not_picked = block_on(host.host.register_folder(scratch.path().join("other"))).unwrap();
    let set = |folder| command(json!({"set_default_location": {"folder": folder}}));

    assert!(bridge.admit(MAIN, &set(folder)).is_ok());
    // The settings window and any other label are refused it before the host sees it.
    for label in [SETTINGS, "other"] {
        let refusal = block_on(bridge.dispatch(label, envelope(set(folder)))).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{label}");
    }
    // A folder ref this run's picker did not issue is refused by the bridge.
    let refusal = bridge.admit(MAIN, &set(not_picked)).unwrap_err();
    assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{refusal:?}");
    // The picked folder is admitted, and the host answers it.
    let outcome = block_on(bridge.dispatch(MAIN, envelope(set(folder))));
    assert!(matches!(outcome, Ok(CommandOutcome::Done)), "{outcome:?}");
}
