//! The map from a detection to the wire's protection, `retry_asks_admin`, and when the view
//! counts as changed (the settings' hook).

use clauseo_protocol::{ErrorCode, NotProtectedCause, Protection, ProtectionTier, UnsupportedWhy};
use clauseo_sandbox::{Blocked, Detection, NoSandbox, SET_UP_BY_OTHER_USER, SetupState, Tier};

use super::{Observed, ProtectionHost, asks_admin, observe, protection_of, refusal_of};

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
        (
            c(NoSandbox::SetUpFailed(SET_UP_BY_OTHER_USER.to_owned())),
            not(NotProtectedCause::OtherAccount),
        ),
        (
            c(NoSandbox::SetUpBlocked(Blocked::OtherAccount)),
            not(NotProtectedCause::OtherAccount),
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

/// Every block but another account's carries the sandbox's own name, unchanged.
#[test]
fn each_block_passes_its_name_through() {
    for blocked in Blocked::ALL {
        let expected = if blocked == Blocked::OtherAccount {
            NotProtectedCause::OtherAccount
        } else {
            NotProtectedCause::Blocked {
                reason: blocked.name().to_owned(),
            }
        };
        let detection = c(NoSandbox::SetUpBlocked(blocked));
        assert_eq!(protection_of(&detection), not(expected), "{blocked:?}");
    }
}

/// The refusal by cause and `sandbox.required`: the four causes of a computer that has the
/// sandbox but can't use it refuse with their codes either way; the rest only when required.
#[test]
fn each_cause_refuses_by_its_code_and_the_setting() {
    let pool_full = NotProtectedCause::PoolFull {
        size: 8,
        open: 8,
        held: 0,
        owed: 0,
        set_aside: 0,
        needs_reset: 0,
        why: None,
    };
    let blocked = NotProtectedCause::Blocked {
        reason: "policy".to_owned(),
    };
    let rows = [
        (
            blocked,
            Some(ErrorCode::SandboxBlocked),
            Some(ErrorCode::SandboxBlocked),
        ),
        (
            NotProtectedCause::OtherAccount,
            Some(ErrorCode::SandboxBlocked),
            Some(ErrorCode::SandboxBlocked),
        ),
        (
            pool_full,
            Some(ErrorCode::SandboxPoolFull),
            Some(ErrorCode::SandboxPoolFull),
        ),
        (
            NotProtectedCause::PoolOwned,
            Some(ErrorCode::SandboxPoolOwned),
            Some(ErrorCode::SandboxPoolOwned),
        ),
        (
            NotProtectedCause::NeedsSetUp,
            Some(ErrorCode::SandboxSetupRequired),
            None,
        ),
        (
            NotProtectedCause::SetUpRefused,
            Some(ErrorCode::SandboxSetupRequired),
            None,
        ),
        // A set-up in place but broken fails closed whatever the setting (R17 PR-E review).
        (
            NotProtectedCause::SetUpFailed,
            Some(ErrorCode::SandboxBlocked),
            Some(ErrorCode::SandboxBlocked),
        ),
        (
            // A cause this host does not know, as a newer sandbox's would decode.
            serde_json::from_value(serde_json::json!("slot_lost")).unwrap(),
            Some(ErrorCode::SandboxBlocked),
            Some(ErrorCode::SandboxBlocked),
        ),
        (
            NotProtectedCause::Unsupported {
                why: UnsupportedWhy::NotBuilt,
            },
            Some(ErrorCode::SandboxRequired),
            None,
        ),
        (
            NotProtectedCause::SwitchedOff {
                at: clauseo_protocol::Timestamp::ZERO,
            },
            Some(ErrorCode::SandboxRequired),
            None,
        ),
    ];
    for (cause, required, not_required) in rows {
        let code = |required| refusal_of(&cause, required).map(|error| error.code());
        assert_eq!(code(true), required, "{cause:?} required");
        assert_eq!(code(false), not_required, "{cause:?} not required");
    }
}

#[test]
fn no_seam_is_not_built_with_no_retry() {
    assert_eq!(
        observe(None, std::path::Path::new("data")),
        Observed {
            computer: not(NotProtectedCause::Unsupported {
                why: UnsupportedWhy::NotBuilt,
            }),
            asks_admin: false,
            set_up_at: None,
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
        // R17 PR-E: no "Try again" for a block; the remedy is IT's or the installer's.
        (SetupState::Blocked(Blocked::Policy), false),
        (SetupState::Blocked(Blocked::OtherAccount), false),
    ] {
        assert_eq!(asks_admin(&state), asks, "{state:?}");
    }
}

/// The loop's settings hook: a change of either setting gives the new view once; no change
/// gives none.
#[test]
fn a_change_of_either_setting_gives_the_new_view_once() {
    let mut host = ProtectionHost::new(None, observe(None, std::path::Path::new("data")));
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
