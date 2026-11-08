//! Tier detection: how strong a jail this machine gives. What the OS can give is probed and
//! cached once it settles (a jail found, or none here); a probe that could not confirm either is
//! never cached, so the next call probes again and a flaky check heals without a restart. On
//! Windows, whether tier B is set up is read on every call (D6), so a set-up that finishes shows
//! without a restart.

use std::sync::OnceLock;

use serde::Serialize;

use crate::SandboxSetup;
#[cfg(any(windows, test))]
use crate::SetupState;

/// The protection tier (synthesis §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Tier {
    /// An OS jail: Seatbelt on macOS (and MXC on Windows, S15a).
    A,
    /// The Windows sandbox user (S15).
    B,
    /// No jail: the matter shows Not protected and the adapters' ask modes apply.
    C,
}

/// Why there is no jail (tier C). R17 maps each to `NotProtectedCause`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NoSandbox {
    /// The jail does not exist here: this OS has none in Clauseo (Linux stays tier C, shared
    /// ruling 6), this Windows has none (ARM64, or a build before Windows 10 22H2), or this
    /// macOS has no `/usr/bin/sandbox-exec`.
    UnsupportedOs,
    /// The jail for this OS is not built yet. Nothing gives it since S15; kept for the wire.
    NotBuilt,
    /// The jail should exist here, but its probe could not confirm it works: macOS'
    /// `sandbox-exec` would not start, failed or hung, or Windows' build can't be read. Fails
    /// closed, and is never cached. What it said, which names no path.
    ProbeFailed(String),
    /// Windows: the sandbox user is not set up for this user, or an older Clauseo set it up.
    NeedsSetUp,
    /// Windows: the set-up's prompt was declined.
    SetUpRefused,
    /// Windows: the set-up failed, its records can't be read, or another Windows user owns it
    /// (D5); what happened.
    SetUpFailed(String),
    /// Windows: a setting only whoever looks after the computer can change blocks the set-up
    /// (D-B4); which one.
    SetUpBlocked(crate::Blocked),
}

/// What [`detect`] found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Detection {
    /// The tier.
    pub tier: Tier,
    /// Why the tier is C; `None` for A and B.
    pub reason: Option<NoSandbox>,
}

impl Detection {
    fn none(reason: NoSandbox) -> Detection {
        Detection {
            tier: Tier::C,
            reason: Some(reason),
        }
    }

    /// Whether this detection may be cached for the process: a jail, or none on this OS. A
    /// failed probe, or any other reason, is probed again at the next call.
    #[cfg(any(not(windows), test))]
    fn settled(&self) -> bool {
        matches!(self.reason, None | Some(NoSandbox::UnsupportedOs))
    }
}

/// `cell`'s value, else `probe`'s, which fills `cell` only when `keep` says it settled: a
/// value that did not is probed again at the next call.
fn cached<T: Clone>(cell: &OnceLock<T>, probe: impl FnOnce() -> T, keep: impl Fn(&T) -> bool) -> T {
    if let Some(value) = cell.get() {
        return value.clone();
    }
    let value = probe();
    if keep(&value) {
        // Another thread may have filled it meanwhile, with a value as good.
        let _filled = cell.set(value.clone());
    }
    value
}

/// The reason [`SetupState::SetUpByOtherUser`] gives, in [`NoSandbox::SetUpFailed`].
pub const SET_UP_BY_OTHER_USER: &str = "set up by another Windows user";

/// The tier of this machine. The OS's part is probed and, once it settles, cached for the
/// process; a failed probe is not cached, so every call probes until one settles. macOS: the
/// Seatbelt probe, as [`probe`] describes, cached when it is tier A or there is no
/// `sandbox-exec`; `setup` is never asked. Windows: the OS's part (architecture and build) is
/// cached once the build reads (or the machine is ARM64), then `setup`'s
/// [`state`](SandboxSetup::state) is read on every call: tier B only when it is
/// [`SetupState::Ready`], else C with its reason; no `setup` is C with
/// [`NoSandbox::NeedsSetUp`]. Others: C with [`NoSandbox::UnsupportedOs`].
pub fn detect(setup: Option<&dyn SandboxSetup>) -> Detection {
    #[cfg(windows)]
    {
        static OS: OnceLock<OsFacts> = OnceLock::new();
        decide(
            &cached(&OS, crate::windows::os_facts, OsFacts::settled),
            setup,
        )
    }
    #[cfg(not(windows))]
    {
        static DETECTED: OnceLock<Detection> = OnceLock::new();
        let _ = setup;
        cached(&DETECTED, || probe(None), Detection::settled)
    }
}

/// The probe behind [`detect`], uncached. macOS: tier A when `/usr/bin/sandbox-exec` runs
/// `/usr/bin/true` under `(version 1)(allow default)` and it exits 0 within 5 seconds; C with
/// [`NoSandbox::UnsupportedOs`] when there is no `sandbox-exec` (`NotFound`); else C with
/// [`NoSandbox::ProbeFailed`] (it exits non-zero, is killed, does not start for another
/// reason, or hangs); `setup` is never asked. Windows: as [`detect`], with the OS read
/// again. Others: C with [`NoSandbox::UnsupportedOs`].
pub fn probe(setup: Option<&dyn SandboxSetup>) -> Detection {
    #[cfg(target_os = "macos")]
    {
        let _ = setup;
        seatbelt::probe(
            std::path::Path::new(seatbelt::SANDBOX_EXEC),
            seatbelt::TIMEOUT,
        )
    }
    #[cfg(windows)]
    {
        decide(&crate::windows::os_facts(), setup)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        let _ = setup;
        Detection::none(NoSandbox::UnsupportedOs)
    }
}

/// What Windows can give, found once per process: the machine's own architecture (an x64 build
/// under ARM64's emulation still sees ARM64) and the build number.
#[cfg(any(windows, test))]
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OsFacts {
    /// The machine is ARM64, which stays tier C until one exists to test on (G6 row 13).
    pub(crate) arm64: bool,
    /// `CurrentBuildNumber`; why it could not be read.
    pub(crate) build: Result<u32, String>,
}

#[cfg(any(windows, test))]
impl OsFacts {
    /// Whether these facts may be cached: the build read, or ARM64, which is tier C whatever
    /// the build. A build that did not read is read again at the next call.
    pub(crate) fn settled(&self) -> bool {
        self.arm64 || self.build.is_ok()
    }
}

/// Windows 10 22H2's build; tier B needs it or later.
#[cfg(any(windows, test))]
pub(crate) const FIRST_BUILD: u32 = 19045;

/// The Windows tier from the cached OS facts and the set-up's live state. ARM64 and builds
/// before [`FIRST_BUILD`] are C with [`NoSandbox::UnsupportedOs`] whatever the set-up says, and a
/// build that can't be read is C with [`NoSandbox::ProbeFailed`] (fail closed, before the
/// set-up is asked, so even a computer never set up refuses until it reads). Tier A (S15a's
/// MXC probe) is not built, so the rest is tier B when the state is [`SetupState::Ready`] and C
/// otherwise.
#[cfg(any(windows, test))]
fn decide(os: &OsFacts, setup: Option<&dyn SandboxSetup>) -> Detection {
    if os.arm64 {
        return Detection::none(NoSandbox::UnsupportedOs);
    }
    match &os.build {
        Ok(build) if *build < FIRST_BUILD => return Detection::none(NoSandbox::UnsupportedOs),
        Ok(_) => {}
        Err(why) => return Detection::none(NoSandbox::ProbeFailed(why.clone())),
    }
    let Some(setup) = setup else {
        return Detection::none(NoSandbox::NeedsSetUp);
    };
    match setup.state() {
        SetupState::Ready => Detection {
            tier: Tier::B,
            reason: None,
        },
        SetupState::Needed | SetupState::Outdated { .. } => Detection::none(NoSandbox::NeedsSetUp),
        SetupState::Refused => Detection::none(NoSandbox::SetUpRefused),
        SetupState::Failed(why) => Detection::none(NoSandbox::SetUpFailed(why)),
        SetupState::Blocked(cause) => Detection::none(NoSandbox::SetUpBlocked(cause)),
        SetupState::SetUpByOtherUser => {
            Detection::none(NoSandbox::SetUpFailed(SET_UP_BY_OTHER_USER.to_owned()))
        }
    }
}

#[cfg(target_os = "macos")]
mod seatbelt;

#[cfg(test)]
mod tests;
