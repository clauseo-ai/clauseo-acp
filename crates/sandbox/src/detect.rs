//! Tier detection: how strong a jail this machine gives. What the OS can give is probed once
//! and cached; on Windows, whether tier B is set up is read on every call (D6), so a set-up that
//! finishes shows without a restart.

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
    /// This OS has no jail in Clauseo (Linux stays tier C, shared ruling 6), or this Windows
    /// has none: ARM64, or a build before Windows 10 22H2.
    UnsupportedOs,
    /// The jail for this OS is not built yet. Nothing gives it since S15; kept for the wire.
    NotBuilt,
    /// The jail's probe failed; what it said.
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
}

/// The reason [`SetupState::SetUpByOtherUser`] gives, in [`NoSandbox::SetUpFailed`].
pub const SET_UP_BY_OTHER_USER: &str = "set up by another Windows user";

/// The tier of this machine. The OS's part is probed on the first call and cached for the
/// process. macOS: the Seatbelt probe, as [`probe`] describes; `setup` is never asked. Windows:
/// the OS's part (architecture and build) is cached, then `setup`'s
/// [`state`](SandboxSetup::state) is read on every call: tier B only when it is
/// [`SetupState::Ready`], else C with its reason; no `setup` is C with
/// [`NoSandbox::NeedsSetUp`]. Others: C with [`NoSandbox::UnsupportedOs`].
pub fn detect(setup: Option<&dyn SandboxSetup>) -> Detection {
    #[cfg(windows)]
    {
        static OS: OnceLock<OsFacts> = OnceLock::new();
        decide(OS.get_or_init(crate::windows::os_facts), setup)
    }
    #[cfg(not(windows))]
    {
        static DETECTED: OnceLock<Detection> = OnceLock::new();
        let _ = setup;
        DETECTED.get_or_init(|| probe(None)).clone()
    }
}

/// The probe behind [`detect`], uncached. macOS: tier A when `/usr/bin/sandbox-exec` runs
/// `/usr/bin/true` under `(version 1)(allow default)` and it exits 0, else C with
/// [`NoSandbox::ProbeFailed`]; `setup` is never asked. Windows: as [`detect`], with the OS read
/// again. Others: C with [`NoSandbox::UnsupportedOs`].
pub fn probe(setup: Option<&dyn SandboxSetup>) -> Detection {
    #[cfg(target_os = "macos")]
    {
        let _ = setup;
        seatbelt::probe(std::path::Path::new(seatbelt::SANDBOX_EXEC))
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

/// Windows 10 22H2's build; tier B needs it or later.
#[cfg(any(windows, test))]
pub(crate) const FIRST_BUILD: u32 = 19045;

/// The Windows tier from the cached OS facts and the set-up's live state. ARM64 and builds
/// before [`FIRST_BUILD`] are C with [`NoSandbox::UnsupportedOs`] whatever the set-up says, and a
/// build that can't be read is C with [`NoSandbox::ProbeFailed`]. Tier A (S15a's MXC probe) is
/// not built, so the rest is tier B when the state is [`SetupState::Ready`] and C otherwise.
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
mod seatbelt {
    use std::io;
    use std::path::Path;
    use std::process::{Command, ExitStatus, Stdio};

    use super::{Detection, NoSandbox, Tier};
    use crate::Error;

    /// The Seatbelt front end; the launcher runs the agent through it too.
    pub(super) const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
    /// A profile that allows everything: the probe asks only whether Seatbelt applies one.
    const PROFILE: &str = "(version 1)(allow default)";
    /// What the probe runs in the jail.
    const TRUE: &str = "/usr/bin/true";

    /// Runs `program` as `sandbox-exec` with the probe profile.
    pub(super) fn probe(program: &Path) -> Detection {
        match run(program) {
            Ok(status) if status.success() => Detection {
                tier: Tier::A,
                reason: None,
            },
            Ok(status) => Detection::none(NoSandbox::ProbeFailed(format!(
                "{} exited with {status}",
                program.display()
            ))),
            Err(error) => {
                let error = Error::Probe(error);
                tracing::warn!(%error, "Seatbelt probe failed");
                Detection::none(NoSandbox::ProbeFailed(format!(
                    "{}: {error}",
                    program.display()
                )))
            }
        }
    }

    #[expect(
        clippy::disallowed_methods,
        reason = "the one-off Seatbelt probe at host start is not an agent launch; it waits for a \
                  process that exits at once"
    )]
    fn run(program: &Path) -> io::Result<ExitStatus> {
        Command::new(program)
            .args(["-p", PROFILE, TRUE])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn a_missing_sandbox_exec_is_tier_c_with_the_reason() {
            let found = probe(Path::new("/nonexistent/sandbox-exec"));
            assert_eq!(found.tier, Tier::C);
            let Some(NoSandbox::ProbeFailed(why)) = found.reason else {
                panic!("expected ProbeFailed, got {found:?}");
            };
            assert!(why.contains("/nonexistent/sandbox-exec"), "{why}");
        }

        #[test]
        fn a_failing_probe_is_tier_c() {
            // `/usr/bin/false` takes the profile's arguments and exits 1.
            let found = probe(Path::new("/usr/bin/false"));
            assert_eq!(found.tier, Tier::C);
            assert!(
                matches!(found.reason, Some(NoSandbox::ProbeFailed(_))),
                "{found:?}"
            );
        }
    }
}

#[cfg(test)]
mod tests;
