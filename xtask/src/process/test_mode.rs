//! The hidden `process-test` mode, which xtask/tests/process.rs runs to kill xtask while a
//! command runs through the door. Not part of the door's API: nothing else in xtask calls it.

use super::running::{self, Finish, Streams};
use super::{Captured, Cmd, Ended, Output, Program, Quiet, capture, run};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

/// How the hidden `process-test` mode runs its script, for xtask/tests/process.rs.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum TestOutput {
    /// Through `run` with `Output::Inherit`.
    Inherit,
    /// Through `capture`.
    Capture,
    /// Started with its output inherited; then, on Unix, the leader is killed, and the script runs
    /// again to its end through `run`, with `$1` set to `again` (which starts a new leader); then
    /// the first run is waited for. So only a new leader told about the first run's group can stop
    /// it when xtask dies.
    RestartedLeader,
}

/// The limit of the hidden `process-test` mode: longer than any test waits, so the test's signal
/// ends the run, not the limit.
const PROCESS_TEST_TIMEOUT: Duration = Duration::from_mins(10);

/// The hidden `process-test` mode: runs `sh -c <script>` through the door as `output` says, so
/// xtask/tests/process.rs can kill this xtask while the script runs. Exits 0 when the script
/// exits 0, 1 otherwise.
pub(crate) fn test_mode(output: TestOutput, script: &str) -> ExitCode {
    let cmd = Cmd::new(Program::Named("sh"), ["-c", script], PROCESS_TEST_TIMEOUT);
    let success = match output {
        TestOutput::Inherit => matches!(
            run(&cmd, Path::new("."), Output::Inherit),
            Ok(Ended::Exited(Quiet { success: true, .. }))
        ),
        TestOutput::Capture => matches!(
            capture(&cmd, Path::new("."), &mut |_| {}),
            Ok(Captured { success: true, .. })
        ),
        TestOutput::RestartedLeader => restarted_leader(&cmd, script),
    };
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// The `restarted-leader` test mode for `cmd`, which is `sh -c <script>`. Whether both runs
/// exited 0.
pub(super) fn restarted_leader(cmd: &Cmd, script: &str) -> bool {
    let mut command = cmd.program.command();
    command.args(&cmd.args);
    let limits = running::Limits::of_cmd(cmd);
    let started = running::Running::start(command, &cmd.to_string(), Streams::Inherit, limits);
    let Ok(first) = started else {
        return false;
    };
    #[cfg(unix)]
    kill_leader();
    let again = Cmd::new(
        Program::Named("sh"),
        ["-c", script, "sh", "again"],
        cmd.timeout,
    );
    let second = run(&again, Path::new("."), Output::Inherit);
    let first = first.finish(&mut |_, _| {});
    matches!(second, Ok(Ended::Exited(Quiet { success: true, .. })))
        && matches!(first, Ok(Finish::Exited(status)) if status.success())
}

/// Kills the leader with SIGKILL and reaps it, leaving it in place, so the next command finds it
/// dead. For the `restarted-leader` mode and the leader's tests only.
#[cfg(unix)]
pub(super) fn kill_leader() {
    let mut slot = super::leader::LEADER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(leader) = slot.as_mut() {
        drop(leader.child.kill());
        drop(leader.child.wait());
    }
}
