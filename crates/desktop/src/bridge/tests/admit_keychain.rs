//! `KeychainReadAgain` is desktop's own step once the keychain is allowed: desktop dispatches
//! it itself, past the window checks, so every window is refused it, before the host sees it.

use serde_json::json;

use super::*;

/// `keychain_read_again`, as a window could write it, is refused from `main`, the settings
/// window and any other label with `command.invalid`, through `admit` and through `dispatch`.
#[test]
fn keychain_read_again_is_refused_from_every_window() {
    let scratch = Scratch::new("admit-keychain", &[]);
    let host = scratch.start(Vec::new());
    let bridge = host.bridge();
    let again: HostCommand = serde_json::from_value(json!({"keychain_read_again": {}})).unwrap();
    assert_eq!(again, HostCommand::KeychainReadAgain {});
    for label in [MAIN, SETTINGS, "other"] {
        let refused = bridge.admit(label, &again).unwrap_err();
        assert_eq!(refused.code(), ErrorCode::CommandInvalid, "{label}");
        let refusal = block_on(bridge.dispatch(label, envelope(again.clone()))).unwrap_err();
        assert_eq!(refusal.code(), ErrorCode::CommandInvalid, "{label}");
    }
}
