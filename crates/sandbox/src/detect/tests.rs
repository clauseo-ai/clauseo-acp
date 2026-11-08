//! Detection: the macOS and Linux probes as before, and Windows' rules through `decide`, which
//! runs on every OS with a fake set-up.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock};

use clauseo_protocol::MatterId;

use super::*;
use crate::SetupError;

/// A set-up whose state the test sets, counting each read.
struct FakeSetup {
    state: Mutex<SetupState>,
    reads: AtomicUsize,
}

impl FakeSetup {
    fn new(state: SetupState) -> FakeSetup {
        FakeSetup {
            state: Mutex::new(state),
            reads: AtomicUsize::new(0),
        }
    }

    fn set(&self, state: SetupState) {
        *self.state.lock().unwrap() = state;
    }

    fn reads(&self) -> usize {
        self.reads.load(Ordering::SeqCst)
    }
}

impl SandboxSetup for FakeSetup {
    fn state(&self) -> SetupState {
        self.reads.fetch_add(1, Ordering::SeqCst);
        self.state.lock().unwrap().clone()
    }

    fn run(&self) -> Result<(), SetupError> {
        Err(SetupError::Refused)
    }

    fn forget_matter(&self, _matter: &MatterId) -> Result<(), SetupError> {
        Ok(())
    }

    fn remove(&self) -> Result<(), SetupError> {
        Ok(())
    }
}

/// An x64 Windows 11 24H2.
fn windows_11() -> OsFacts {
    OsFacts {
        arm64: false,
        build: Ok(26100),
    }
}

fn tier_b() -> Detection {
    Detection {
        tier: Tier::B,
        reason: None,
    }
}

/// Off macOS and Windows the detection settles (no jail here), so it is cached.
#[test]
fn detection_is_cached_and_matches_the_probe() {
    assert_eq!(detect(None), probe(None));
    assert_eq!(detect(None), detect(None));
}

#[test]
fn a_failed_probe_is_never_cached_and_a_success_is() {
    let cell = OnceLock::new();
    let probes = AtomicUsize::new(0);
    let failed = || {
        probes.fetch_add(1, Ordering::SeqCst);
        Detection::none(NoSandbox::ProbeFailed(
            "sandbox-exec exited with 1".to_owned(),
        ))
    };
    for _ in 0..2 {
        let found = cached(&cell, failed, Detection::settled);
        assert!(matches!(found.reason, Some(NoSandbox::ProbeFailed(_))));
    }
    assert_eq!(probes.load(Ordering::SeqCst), 2, "each call probes again");
    let tier_a = Detection {
        tier: Tier::A,
        reason: None,
    };
    let ok = || {
        probes.fetch_add(1, Ordering::SeqCst);
        tier_a.clone()
    };
    assert_eq!(cached(&cell, ok, Detection::settled), tier_a);
    assert_eq!(cached(&cell, ok, Detection::settled), tier_a);
    assert_eq!(probes.load(Ordering::SeqCst), 3, "the success is cached");
}

#[test]
fn only_a_jail_or_no_jail_here_settles() {
    let tier_a = Detection {
        tier: Tier::A,
        reason: None,
    };
    assert!(tier_a.settled());
    assert!(Detection::none(NoSandbox::UnsupportedOs).settled());
    assert!(!Detection::none(NoSandbox::ProbeFailed("x".to_owned())).settled());
    assert!(!Detection::none(NoSandbox::NeedsSetUp).settled());
}

#[test]
fn windows_facts_are_cached_only_once_the_build_reads() {
    let unread = OsFacts {
        arm64: false,
        build: Err("CurrentBuildNumber: not a number".to_owned()),
    };
    assert!(!unread.settled());
    assert!(windows_11().settled());
    let arm = OsFacts {
        arm64: true,
        build: Err("x".to_owned()),
    };
    assert!(arm.settled(), "ARM64 is tier C whatever the build");
    let cell = OnceLock::new();
    let reads = AtomicUsize::new(0);
    let facts = [unread, windows_11()];
    let read = || facts[reads.fetch_add(1, Ordering::SeqCst).min(1)].clone();
    assert!(cached(&cell, read, OsFacts::settled).build.is_err());
    assert_eq!(cached(&cell, read, OsFacts::settled), windows_11());
    assert_eq!(cached(&cell, read, OsFacts::settled), windows_11());
    assert_eq!(
        reads.load(Ordering::SeqCst),
        2,
        "the unread build was read again, once"
    );
}

#[test]
fn arm64_is_tier_c_unsupported_even_when_set_up() {
    let os = OsFacts {
        arm64: true,
        build: Ok(26100),
    };
    let setup = FakeSetup::new(SetupState::Ready);
    assert_eq!(
        decide(&os, Some(&setup)),
        Detection::none(NoSandbox::UnsupportedOs)
    );
    assert_eq!(decide(&os, None), Detection::none(NoSandbox::UnsupportedOs));
}

#[test]
fn builds_before_windows_10_22h2_are_unsupported() {
    let setup = FakeSetup::new(SetupState::Ready);
    for build in [17763, 19041, FIRST_BUILD - 1] {
        let os = OsFacts {
            arm64: false,
            build: Ok(build),
        };
        assert_eq!(
            decide(&os, Some(&setup)),
            Detection::none(NoSandbox::UnsupportedOs),
            "{build}"
        );
    }
    for build in [FIRST_BUILD, 20348, 22000, 26100] {
        let os = OsFacts {
            arm64: false,
            build: Ok(build),
        };
        assert_eq!(decide(&os, Some(&setup)), tier_b(), "{build}");
    }
}

/// An unreadable build fails closed as a failed probe, before the set-up is asked: never a
/// set-up's state, even on a computer never set up.
#[test]
fn an_unreadable_build_is_a_failed_probe() {
    let os = OsFacts {
        arm64: false,
        build: Err("CurrentBuildNumber: not a number".to_owned()),
    };
    let setup = FakeSetup::new(SetupState::Ready);
    assert_eq!(
        decide(&os, Some(&setup)),
        Detection::none(NoSandbox::ProbeFailed(
            "CurrentBuildNumber: not a number".to_owned()
        ))
    );
    assert_eq!(setup.reads(), 0, "the set-up is not asked on an unknown OS");
    let never = FakeSetup::new(SetupState::Needed);
    assert!(matches!(
        decide(&os, Some(&never)).reason,
        Some(NoSandbox::ProbeFailed(_))
    ));
    assert!(matches!(
        decide(&os, None).reason,
        Some(NoSandbox::ProbeFailed(_))
    ));
}

#[test]
fn each_setup_state_gives_its_tier_or_reason() {
    let cases = [
        (SetupState::Ready, tier_b()),
        (SetupState::Needed, Detection::none(NoSandbox::NeedsSetUp)),
        (
            SetupState::Outdated {
                found: 1,
                current: 2,
            },
            Detection::none(NoSandbox::NeedsSetUp),
        ),
        (
            SetupState::Refused,
            Detection::none(NoSandbox::SetUpRefused),
        ),
        (
            SetupState::Failed("setup.json: bad".to_owned()),
            Detection::none(NoSandbox::SetUpFailed("setup.json: bad".to_owned())),
        ),
        (
            SetupState::SetUpByOtherUser,
            Detection::none(NoSandbox::SetUpFailed(SET_UP_BY_OTHER_USER.to_owned())),
        ),
        (
            SetupState::Blocked(crate::Blocked::LogonRight),
            Detection::none(NoSandbox::SetUpBlocked(crate::Blocked::LogonRight)),
        ),
        (
            SetupState::Blocked(crate::Blocked::Policy),
            Detection::none(NoSandbox::SetUpBlocked(crate::Blocked::Policy)),
        ),
    ];
    for (state, expected) in cases {
        let setup = FakeSetup::new(state.clone());
        assert_eq!(decide(&windows_11(), Some(&setup)), expected, "{state:?}");
    }
}

#[test]
fn no_setup_on_a_supported_windows_needs_set_up() {
    assert_eq!(
        decide(&windows_11(), None),
        Detection::none(NoSandbox::NeedsSetUp)
    );
}

#[test]
fn the_setup_state_is_read_on_every_call() {
    let os = windows_11();
    let setup = FakeSetup::new(SetupState::Needed);
    assert_eq!(
        decide(&os, Some(&setup)),
        Detection::none(NoSandbox::NeedsSetUp)
    );
    setup.set(SetupState::Ready);
    assert_eq!(decide(&os, Some(&setup)), tier_b());
    setup.set(SetupState::Failed("gone".to_owned()));
    assert_eq!(
        decide(&os, Some(&setup)),
        Detection::none(NoSandbox::SetUpFailed("gone".to_owned()))
    );
    assert_eq!(setup.reads(), 3);
}

#[test]
fn the_new_reasons_keep_their_wire_names() {
    let names = [
        (NoSandbox::NeedsSetUp, "\"needs_set_up\""),
        (NoSandbox::SetUpRefused, "\"set_up_refused\""),
        (
            NoSandbox::SetUpFailed("x".to_owned()),
            "{\"set_up_failed\":\"x\"}",
        ),
        (
            NoSandbox::SetUpBlocked(crate::Blocked::Policy),
            "{\"set_up_blocked\":\"policy\"}",
        ),
        (
            NoSandbox::SetUpBlocked(crate::Blocked::LogonRight),
            "{\"set_up_blocked\":\"logon_right\"}",
        ),
    ];
    for (reason, json) in names {
        assert_eq!(serde_json::to_string(&reason).unwrap(), json);
    }
}

/// On Windows `detect` itself follows the set-up: the OS part is cached, the state is not.
#[cfg(windows)]
#[test]
fn windows_detection_follows_the_setup_between_calls() {
    let setup = FakeSetup::new(SetupState::Needed);
    let first = detect(Some(&setup));
    setup.set(SetupState::Ready);
    let second = detect(Some(&setup));
    if first.reason == Some(NoSandbox::NeedsSetUp) {
        // A supported machine (every CI runner): the second call sees the new state.
        assert_eq!(second, tier_b());
        assert_eq!(setup.reads(), 2);
    } else {
        // ARM64 or an old build: never asks.
        assert_eq!(first, second);
        assert_eq!(setup.reads(), 0);
    }
}

/// macOS and Linux never ask a set-up.
#[cfg(not(windows))]
#[test]
fn other_systems_never_ask_the_setup() {
    let setup = FakeSetup::new(SetupState::Ready);
    assert_eq!(detect(Some(&setup)), probe(None));
    assert_eq!(probe(Some(&setup)), probe(None));
    assert_eq!(setup.reads(), 0);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_is_tier_a() {
    assert_eq!(
        probe(None),
        Detection {
            tier: Tier::A,
            reason: None
        }
    );
}

#[cfg(not(any(target_os = "macos", windows)))]
#[test]
fn other_systems_are_unsupported() {
    assert_eq!(probe(None), Detection::none(NoSandbox::UnsupportedOs));
}
