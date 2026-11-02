//! Tier detection: how strong a jail this machine gives, found once and cached.

use std::sync::OnceLock;

use serde::Serialize;

/// The protection tier (synthesis §2.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Tier {
    /// An OS jail: Seatbelt on macOS (and MXC on Windows, S15).
    A,
    /// The Windows sandbox user (S15).
    B,
    /// No jail: the matter shows Not protected and the adapters' ask modes apply.
    C,
}

/// Why there is no jail (tier C). S15 adds the Windows set-up causes; R17 maps each to
/// `NotProtectedCause`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum NoSandbox {
    /// This OS has no jail in Clauseo (Linux stays tier C, shared ruling 6).
    UnsupportedOs,
    /// The jail for this OS is not built yet (Windows until S15).
    NotBuilt,
    /// The jail's probe failed; what it said.
    ProbeFailed(String),
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

/// The tier of this machine, probed on the first call and cached for the process (the host
/// calls it at start). [`probe`] runs the probe again, uncached.
pub fn detect() -> Detection {
    static DETECTED: OnceLock<Detection> = OnceLock::new();
    DETECTED.get_or_init(probe).clone()
}

/// The probe behind [`detect`], uncached. macOS: tier A when `/usr/bin/sandbox-exec` runs
/// `/usr/bin/true` under `(version 1)(allow default)` and it exits 0, else C with
/// [`NoSandbox::ProbeFailed`]. Windows: C with [`NoSandbox::NotBuilt`]. Others: C with
/// [`NoSandbox::UnsupportedOs`].
pub fn probe() -> Detection {
    #[cfg(target_os = "macos")]
    {
        seatbelt::probe(std::path::Path::new(seatbelt::SANDBOX_EXEC))
    }
    #[cfg(windows)]
    {
        Detection::none(NoSandbox::NotBuilt)
    }
    #[cfg(not(any(target_os = "macos", windows)))]
    {
        Detection::none(NoSandbox::UnsupportedOs)
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
mod tests {
    use super::*;

    #[test]
    fn detection_is_cached_and_matches_the_probe() {
        assert_eq!(detect(), probe());
        assert_eq!(detect(), detect());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_is_tier_a() {
        assert_eq!(
            probe(),
            Detection {
                tier: Tier::A,
                reason: None
            }
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_is_not_built_yet() {
        assert_eq!(probe(), Detection::none(NoSandbox::NotBuilt));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn other_systems_are_unsupported() {
        assert_eq!(probe(), Detection::none(NoSandbox::UnsupportedOs));
    }
}
