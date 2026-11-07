//! The map from a detection to the wire's protection, `retry_asks_admin`, and when the view
//! counts as changed (the settings' hook).

use clauseo_protocol::{NotProtectedCause, Protection, ProtectionTier, UnsupportedWhy};
use clauseo_sandbox::{Detection, NoSandbox, SetupState, Tier};

use super::{Observed, ProtectionHost, asks_admin, observe, protection_of};

fn c(reason: NoSandbox) -> Detection {
    Detection {
        tier: Tier::C,
        reason: Some(reason),
    }
}

fn not(cause: NotProtectedCause) -> Protection {
    Protection::NotProtected { cause }
}

#[test]
fn each_detection_maps_to_its_protection() {
    let os = if cfg!(all(windows, target_arch = "aarch64")) {
        UnsupportedWhy::WindowsArm
    } else {
        UnsupportedWhy::Os
    };
    let rows = [
        (
            Detection {
                tier: Tier::A,
                reason: None,
            },
            Protection::Protected {
                tier: ProtectionTier::A,
            },
        ),
        (
            Detection {
                tier: Tier::B,
                reason: None,
            },
            Protection::Protected {
                tier: ProtectionTier::B,
            },
        ),
        (
            c(NoSandbox::UnsupportedOs),
            not(NotProtectedCause::Unsupported { why: os }),
        ),
        (
            c(NoSandbox::NotBuilt),
            not(NotProtectedCause::Unsupported {
                why: UnsupportedWhy::NotBuilt,
            }),
        ),
        (
            c(NoSandbox::ProbeFailed(
                "/usr/bin/sandbox-exec: gone".to_owned(),
            )),
            not(NotProtectedCause::SetUpFailed),
        ),
        (
            c(NoSandbox::SetUpFailed("C:\\Users\\x".to_owned())),
            not(NotProtectedCause::SetUpFailed),
        ),
        (c(NoSandbox::NeedsSetUp), not(NotProtectedCause::NeedsSetUp)),
        (
            c(NoSandbox::SetUpRefused),
            not(NotProtectedCause::SetUpRefused),
        ),
        // A tier C with no reason is a contradiction: it fails closed.
        (
            Detection {
                tier: Tier::C,
                reason: None,
            },
            not(NotProtectedCause::SetUpFailed),
        ),
    ];
    for (detection, expected) in rows {
        assert_eq!(protection_of(&detection), expected, "{detection:?}");
    }
}

#[test]
fn no_seam_is_not_built_with_no_retry() {
    assert_eq!(
        observe(None),
        Observed {
            computer: not(NotProtectedCause::Unsupported {
                why: UnsupportedWhy::NotBuilt,
            }),
            asks_admin: false,
        }
    );
}

#[test]
fn a_retry_asks_for_an_administrator_only_when_a_set_up_can_help() {
    for (state, asks) in [
        (SetupState::Needed, true),
        (SetupState::Refused, true),
        (SetupState::Failed("x".to_owned()), true),
        (
            SetupState::Outdated {
                found: 1,
                current: 2,
            },
            true,
        ),
        (SetupState::Ready, false),
        (SetupState::SetUpByOtherUser, false),
    ] {
        assert_eq!(asks_admin(&state), asks, "{state:?}");
    }
}

/// The loop's settings hook: a change of either setting gives the new view once; no change
/// gives none.
#[test]
fn a_change_of_either_setting_gives_the_new_view_once() {
    let mut host = ProtectionHost::new(None, observe(None));
    let first = host.take_changed(false, false).unwrap();
    assert!(!first.required && !first.unprotected_auto_allowed);
    assert!(!host.settings_moved(false, false));
    assert_eq!(host.take_changed(false, false), None);

    assert!(host.settings_moved(true, false));
    let required = host.take_changed(true, false).unwrap();
    assert!(required.required && !required.unprotected_auto_allowed);
    assert!(!host.settings_moved(true, false));

    assert!(host.settings_moved(true, true));
    let auto = host.take_changed(true, true).unwrap();
    assert!(auto.unprotected_auto_allowed);
    assert_eq!(host.take_changed(true, true), None);
}
