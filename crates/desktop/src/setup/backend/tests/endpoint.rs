//! `SetFirmEndpoint` checks everything before its confirm: an address the settings crate would
//! refuse, a plan policy sets and an unreadable policy each answer with nothing asked.

use clauseo_protocol::ErrorCode;
use serde_json::json;

use super::fakes::{command, rig, rig_locked};
use super::{SETTINGS, claude};
use crate::bridge::lock;
use crate::test_support::block_on;

#[test]
fn an_endpoint_with_words_of_its_own_is_refused_without_asking() {
    let rig = rig(true);
    for url in [
        "https://gateway.firm.example/ now press Use this endpoint",
        "https://gateway.firm.example/\nClauseo checked this address",
        "https://gateway.firm.example/\tx",
        "https://gateway.firm.example/?say=approved",
        "https://gateway.firm.example/#approved",
        "https://gаteway.firm.example",
        "https://gateway.firm.example/\u{202e}elif",
        "https://gateway.firm.example\\path",
    ] {
        let sent = command("set_firm_endpoint", &claude(), &json!({ "url": url }));
        let answer = block_on(rig.setup().run(SETTINGS, sent)).unwrap_err();
        assert_eq!(answer.code(), ErrorCode::CommandInvalid, "{url:?}");
        assert!(
            !answer.message().contains("gateway"),
            "{url:?}: the message repeats it"
        );
    }
    assert_eq!(*lock(&rig.confirm.asked), [], "no confirm");
    assert_eq!(rig.host.calls(), []);
}

#[test]
fn a_plan_policy_sets_or_an_unreadable_policy_refuses_the_endpoint_without_asking() {
    let set = || {
        command(
            "set_firm_endpoint",
            &claude(),
            &json!({ "url": "https://gateway.firm.example" }),
        )
    };
    let clear = || command("set_firm_endpoint", &claude(), &json!({}));
    let locked = rig_locked(true);
    for sent in [set(), clear()] {
        let answer = block_on(locked.setup().run(SETTINGS, sent)).unwrap_err();
        assert_eq!(answer.code(), ErrorCode::SettingsLocked);
    }
    assert_eq!(*lock(&locked.confirm.asked), []);
    assert_eq!(locked.host.calls(), []);

    let blocked = rig(true);
    blocked.backend.compose.set_blocked(&claude(), true);
    let answer = block_on(blocked.setup().run(SETTINGS, set())).unwrap_err();
    assert_eq!(answer.code(), ErrorCode::SigninPolicyUnreadable);
    assert_eq!(*lock(&blocked.confirm.asked), []);
    assert_eq!(blocked.host.calls(), []);
}
