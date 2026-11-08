//! macOS' Seatbelt probe: `sandbox-exec` runs `/usr/bin/true` under a profile that allows
//! everything. Missing (`NotFound`) is no jail here, `UnsupportedOs`; anything else short of a
//! clean exit within [`TIMEOUT`] is `ProbeFailed`, which fails closed. Its text names no path.

use std::io;
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

use super::{Detection, NoSandbox, Tier};
use crate::Error;

/// The Seatbelt front end; the launcher runs the agent through it too.
pub(super) const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";
/// How long the probe may run before it counts as hung, is killed and fails.
pub(super) const TIMEOUT: Duration = Duration::from_secs(5);
/// How often the probe's exit is looked for.
const POLL: Duration = Duration::from_millis(10);
/// A profile that allows everything: the probe asks only whether Seatbelt applies one.
const PROFILE: &str = "(version 1)(allow default)";
/// What the probe runs in the jail.
const TRUE: &str = "/usr/bin/true";

/// How the run ended.
#[derive(Debug)]
enum Ran {
    /// It exited, or a signal ended it.
    Exited(ExitStatus),
    /// It did not start: `program` is missing.
    Missing,
    /// It did not start for another reason (permission, resources), or its wait failed.
    Failed(io::Error),
    /// It was still running at the timeout, and was killed.
    Hung,
}

/// Runs `program` as `sandbox-exec` with the probe profile, for at most `timeout`.
pub(super) fn probe(program: &Path, timeout: Duration) -> Detection {
    match run(program, timeout) {
        Ran::Exited(status) if status.success() => Detection {
            tier: Tier::A,
            reason: None,
        },
        Ran::Exited(status) => {
            tracing::warn!(%status, "the Seatbelt probe failed");
            Detection::none(NoSandbox::ProbeFailed(format!(
                "sandbox-exec exited with {status}"
            )))
        }
        Ran::Missing => {
            tracing::info!("no sandbox-exec on this Mac: no Seatbelt here");
            Detection::none(NoSandbox::UnsupportedOs)
        }
        Ran::Failed(error) => {
            let kind = error.kind();
            let error = Error::Probe(error);
            tracing::warn!(%error, "the Seatbelt probe failed");
            Detection::none(NoSandbox::ProbeFailed(format!(
                "sandbox-exec did not run: {kind:?}"
            )))
        }
        Ran::Hung => {
            tracing::warn!(?timeout, "the Seatbelt probe hung and was killed");
            Detection::none(NoSandbox::ProbeFailed(format!(
                "sandbox-exec did not exit within {timeout:?}"
            )))
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the Seatbelt probe is not an agent launch, and runs off the host's actor: it \
              waits, polling, for a process that exits at once, and kills one that hangs"
)]
fn run(program: &Path, timeout: Duration) -> Ran {
    let spawned = Command::new(program)
        .args(["-p", PROFILE, TRUE])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ran::Missing,
        Err(error) => return Ran::Failed(error),
    };
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return Ran::Exited(status),
            Ok(None) => {}
            Err(error) => {
                end(&mut child);
                return Ran::Failed(error);
            }
        }
        if Instant::now() >= deadline {
            end(&mut child);
            return Ran::Hung;
        }
        std::thread::sleep(POLL);
    }
}

/// Kills `child` and reaps it; one already gone is fine.
fn end(child: &mut Child) {
    let _gone = child.kill();
    let _reaped = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{TempDir, set_mode};

    #[test]
    fn a_missing_sandbox_exec_is_unsupported() {
        let found = probe(Path::new("/nonexistent/sandbox-exec"), TIMEOUT);
        assert_eq!(found, Detection::none(NoSandbox::UnsupportedOs));
    }

    #[test]
    fn a_failing_probe_is_a_failed_probe_naming_no_path() {
        // `/usr/bin/false` takes the profile's arguments and exits 1.
        let found = probe(Path::new("/usr/bin/false"), TIMEOUT);
        assert_eq!(found.tier, Tier::C);
        let Some(NoSandbox::ProbeFailed(why)) = found.reason else {
            panic!("expected ProbeFailed, got {found:?}");
        };
        assert!(!why.contains('/'), "{why}");
    }

    #[test]
    fn a_sandbox_exec_that_cannot_be_run_is_a_failed_probe() {
        let temp = TempDir::new("seatbelt-not-executable");
        let program = temp.file("sandbox-exec", "#!/bin/sh\nexit 0\n");
        set_mode(&program, 0o600);
        let found = probe(&program, TIMEOUT);
        assert!(
            matches!(found.reason, Some(NoSandbox::ProbeFailed(_))),
            "{found:?}"
        );
    }

    /// A probe killed by a signal (no exit code) is a failed probe.
    #[test]
    fn a_probe_killed_by_a_signal_is_a_failed_probe() {
        let temp = TempDir::new("seatbelt-killed");
        let program = temp.file("sandbox-exec", "#!/bin/sh\nkill -9 $\n");
        set_mode(&program, 0o700);
        let found = probe(&program, TIMEOUT);
        let Some(NoSandbox::ProbeFailed(why)) = found.reason else {
            panic!("expected ProbeFailed, got {found:?}");
        };
        assert!(why.contains("signal"), "{why}");
    }

    /// A fake that sleeps 60 s: the probe returns, killed at the injected 200 ms, as hung.
    #[test]
    fn a_hanging_probe_times_out_as_a_failed_probe() {
        let temp = TempDir::new("seatbelt-hangs");
        let program = temp.file("sandbox-exec", "#!/bin/sh\nexec /bin/sleep 60\n");
        set_mode(&program, 0o700);
        let found = probe(&program, Duration::from_millis(200));
        let Some(NoSandbox::ProbeFailed(why)) = found.reason else {
            panic!("expected ProbeFailed, got {found:?}");
        };
        assert_eq!(why, "sandbox-exec did not exit within 200ms");
    }

    #[test]
    fn the_real_timeout_is_about_five_seconds() {
        assert_eq!(TIMEOUT, Duration::from_secs(5));
    }
}
