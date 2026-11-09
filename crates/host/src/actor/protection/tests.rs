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

/// A seam that never launches: only its presence counts here.
struct NoLaunch;

impl crate::config::SandboxLaunchers for NoLaunch {
    fn reserve(
        &self,
        matter: &clauseo_protocol::MatterId,
        session: &crate::config::SessionKey,
        _running: &[crate::config::SessionKey],
    ) -> Result<crate::config::sandbox::Reservation, clauseo_sandbox::Error> {
        Ok(crate::config::sandbox::Reservation::unpooled(
            *matter, *session,
        ))
    }

    fn launcher(
        &self,
        _policy: clauseo_sandbox::Policy,
        _matter: &clauseo_protocol::MatterId,
        _session: &crate::config::SessionKey,
        _reservation: crate::config::sandbox::Reservation,
        _opts: clauseo_process::SpawnOptions,
        _denials: std::sync::Arc<dyn clauseo_sandbox::DenialSink>,
    ) -> Result<crate::config::SandboxLaunch, clauseo_sandbox::Error> {
        Err(clauseo_sandbox::Error::Policy("never".to_owned()))
    }

    fn after_session(&self, _end: crate::config::SessionEnd) -> Vec<clauseo_sandbox::SetAside> {
        Vec::new()
    }
}

/// An agent's tier is the computer's only when its launch goes through the sandbox's launchers:
/// without them it is C, even where the computer reports B (#712's review LOW).
#[test]
fn an_agent_s_tier_is_c_without_launchers_and_the_computer_s_with_them() {
    let protected = |tier| Observed {
        computer: Protection::Protected { tier },
        asks_admin: false,
    };
    let detect: crate::config::Detect = std::sync::Arc::new(|| Detection {
        tier: Tier::B,
        reason: None,
    });
    let bare = crate::config::Sandbox::new(std::sync::Arc::clone(&detect), None);
    let jailed = bare.clone().with_launchers(
        std::sync::Arc::new(NoLaunch),
        clauseo_process::SpawnOptions::new(),
    );
    for tier in [ProtectionTier::A, ProtectionTier::B] {
        let without = ProtectionHost::new(Some(bare.clone()), protected(tier));
        assert_eq!(without.launch_tier(), None, "no launchers: tier C");
        let with = ProtectionHost::new(Some(jailed.clone()), protected(tier));
        assert_eq!(with.launch_tier(), Some(tier));
    }
    let unprotected = Observed {
        computer: not(NotProtectedCause::NeedsSetUp),
        asks_admin: false,
    };
    assert_eq!(
        ProtectionHost::new(Some(jailed), unprotected).launch_tier(),
        None
    );
    assert_eq!(
        ProtectionHost::new(None, protected(ProtectionTier::B)).launch_tier(),
        None
    );
}
