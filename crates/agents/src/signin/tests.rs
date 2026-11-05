//! The sign-in types: the codes' strings, and a pasted code that no `Debug` shows.

use std::time::SystemTime;

use zeroize::Zeroizing;

use super::{PasteCode, SigninCode, SigninState, Source};

#[test]
fn each_code_has_its_string() {
    let table = [
        (SigninCode::Cancelled, "signin.cancelled"),
        (SigninCode::LinkExpired, "signin.link_expired"),
        (SigninCode::NoPlan, "signin.no_plan"),
        (SigninCode::DeviceCodeOff, "signin.device_code_off"),
        (SigninCode::PortBusy, "signin.port_busy"),
        (SigninCode::Failed, "signin.failed"),
    ];
    for (code, text) in table {
        assert_eq!(code.code(), text);
    }
}

#[test]
fn debug_never_shows_a_pasted_code() {
    let secret = "s3cret-paste-code-0042";
    let code = PasteCode::new(Zeroizing::new(secret.to_owned()));
    assert_eq!(format!("{code:?}"), "<code>");
    assert_eq!(format!("{code:#?}"), "<code>");
    // Inside another value's Debug too.
    let wrapped = (Some(&code), SigninState::Queued);
    assert!(!format!("{wrapped:?}").contains(secret));
    assert_eq!(&code.line()[..], b"s3cret-paste-code-0042\n");
}

#[test]
fn a_pasted_code_is_one_line() {
    let code = PasteCode::from("  ab\ncd\r\u{7f}ef \n".to_owned());
    assert_eq!(&code.line()[..], b"abcdef\n");
}

#[test]
fn signed_in_carries_its_source() {
    let state = SigninState::SignedIn {
        email: Some("test@example.com".to_owned()),
        plan: Some("max".to_owned()),
        source: Source::Reused,
        at: SystemTime::UNIX_EPOCH,
    };
    assert!(matches!(
        state,
        SigninState::SignedIn {
            source: Source::Reused,
            ..
        }
    ));
}
