//! The hidden `process-test` mode, which xtask/tests/process.rs runs to kill xtask while a
//! command runs through the door. Not part of the door's API: nothing else in xtask calls it.
//!
//! It prints `REGISTERED` on stdout once the leader has been told the group of every command it
//! started, so a test that kills xtask after that line never kills it inside the window leader.rs
//! lists as not covered (from a command's start to its `+<group>` write).

use super::running::{self, Finish, Running, Streams};
use super::{Cmd, Output, Program, Ran, run};
use crate::output::Printer;
use std::io;
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

/// How the hidden `process-test` mode runs its script, for xtask/tests/process.rs.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub(crate) enum TestOutput {
    /// With the streams `run` gives `Output::Inherit`.
    Inherit,
    /// With the streams `capture` gives its command.
    Capture,
    /// Started with its output inherited; then, on Unix, the leader is killed, and the script runs
    /// again to its end through `run`, with `$1` set to `again` (which starts a new leader); then
    /// the first run is waited for. So only a new leader told about the first run's group can stop
    /// it when xtask dies.
    RestartedLeader,
}

/// The line `process-test` prints once the groups of its commands are registered with the
/// leader. xtask/tests/process.rs spells it too; every test there fails when the two differ.
const REGISTERED: &str = "registered";

/// The limit of the hidden `process-test` mode: longer than any test waits, so the test's signal
/// ends the run, not the limit.
const PROCESS_TEST_TIMEOUT: Duration = Duration::from_mins(10);

/// The hidden `process-test` mode: runs `sh -c <script>` through the door as `output` says, so
/// xtask/tests/process.rs can kill this xtask while the script runs, and prints `REGISTERED` once
/// the leader knows the script's group. Exits 0 when the script exits 0, 1 otherwise.
pub(crate) fn test_mode(output: TestOutput, script: &str) -> ExitCode {
    let cmd = Cmd::new(Program::Named("sh"), ["-c", script], PROCESS_TEST_TIMEOUT);
    let mut registered = || Printer::stdout().note(REGISTERED);
    let success = match output {
        // `run` and `capture` return only once the command has ended, so these take their steps
        // one by one: `Streams::Inherit` is what `run` passes for `Output::Inherit`,
        // `Streams::Merged` what `capture` passes.
        TestOutput::Inherit => run_noting(&cmd, Streams::Inherit, &mut registered),
        TestOutput::Capture => run_noting(&cmd, Streams::Merged, &mut registered),
        TestOutput::RestartedLeader => restarted_leader(&cmd, script, &mut registered),
    };
    if success {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Starts `cmd` with `streams`, as `running::run` does. On Unix `Running::start` returns only
/// once the leader has been told the command's group.
fn start(cmd: &Cmd, streams: Streams) -> io::Result<Running> {
    let mut command = cmd.program.command();
    command.args(&cmd.args);
    Running::start(
        command,
        &cmd.to_string(),
        streams,
        running::Limits::of_cmd(cmd),
    )
}

/// Runs `cmd` with `streams`, calling `registered` once it has started. Whether it exited 0.
pub(super) fn run_noting(cmd: &Cmd, streams: Streams, registered: &mut dyn FnMut()) -> bool {
    let Ok(running) = start(cmd, streams) else {
        return false;
    };
    registered();
    matches!(running.finish(&mut |_, _| {}), Ok(Finish::Exited(status)) if status.success())
}

/// The `restarted-leader` test mode for `cmd`, which is `sh -c <script>`, calling `registered`
/// once the second run has ended: its start told the new leader the first run's group and its
/// own. Whether both runs exited 0.
pub(super) fn restarted_leader(cmd: &Cmd, script: &str, registered: &mut dyn FnMut()) -> bool {
    let Ok(first) = start(cmd, Streams::Inherit) else {
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
    registered();
    let first = first.finish(&mut |_, _| {});
    second.as_ref().is_ok_and(Ran::success)
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
