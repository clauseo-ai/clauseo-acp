//! A running agent: its exit, the shutdown ladder, and the tree kill that follows either.

use std::future::Future;
use std::io;
use std::pin::{Pin, pin};
use std::process::ExitStatus;
use std::sync::Arc;
use std::time::Duration;

use async_process::Child;
use clauseo_protocol::DetachReason;
use futures::future::{Either, select};

use crate::Timer;
use crate::os::Tree;
use crate::stderr::{self, Redactor, SharedRing};
use crate::stdin::StdinSlot;

/// How long shutdown waits after closing stdin (ACP agents exit when stdin ends).
const STDIN_GRACE: Duration = Duration::from_secs(2);
/// How long shutdown waits after SIGTERM, before SIGKILL.
#[cfg(unix)]
const TERM_GRACE: Duration = Duration::from_secs(3);
/// How long shutdown waits for the exit after the kill, before it reports the agent unresponsive.
const KILL_GRACE: Duration = Duration::from_secs(2);

/// An agent's process and its tree. Dropping it kills the tree at once, without waiting.
#[derive(Debug)]
pub struct Handle {
    child: Child,
    tree: Tree,
    stdin: StdinSlot,
    stderr: StderrSource,
    /// Whether the tree has been sent its kill, so `Drop` sends none: on Unix a group id can be
    /// reused once the group is gone.
    tree_killed: bool,
}

impl Handle {
    pub(crate) fn new(
        child: Child,
        tree: Tree,
        stdin: StdinSlot,
        ring: SharedRing,
        redactor: Arc<Redactor>,
    ) -> Handle {
        Handle {
            child,
            tree,
            stdin,
            stderr: StderrSource { ring, redactor },
            tree_killed: false,
        }
    }

    /// The agent's process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Waits for the agent to exit on its own, then kills what is left of its tree, so a helper
    /// it started cannot outlive it. Dropping the future and calling again is fine.
    pub async fn exited(&mut self) -> ExitReport {
        let status = self.child.status().await;
        self.finish(&status)
    }

    /// Stops the agent by the ladder: close its stdin and wait 2 s, then (Unix) SIGTERM to its
    /// group and wait 3 s, then SIGKILL to the group, or on Windows end its job, and wait 2 s
    /// more for the exit. Whichever way it exits, what is left of its tree is killed. The outcome
    /// names the step that ended it, or says the agent never exited.
    ///
    /// Keep reading the agent's stdout, or drop it, while this runs: an agent blocked writing to
    /// a full stdout pipe never sees its stdin close, and costs the whole 2 s and the SIGTERM.
    pub async fn shutdown<T: Timer>(mut self, timer: &T) -> ShutdownOutcome {
        self.stdin.close();
        let exit = pin!(self.child.status());
        let ended = ladder(exit, timer, |step| self.step(step)).await;
        match ended {
            Ended::StdinClosed(status) => ShutdownOutcome::StdinClosed(self.finish(&status)),
            #[cfg(unix)]
            Ended::Terminated(status) => ShutdownOutcome::Terminated(self.finish(&status)),
            Ended::Killed(status) => ShutdownOutcome::Killed(self.finish(&status)),
            Ended::Unresponsive => ShutdownOutcome::Unresponsive(self.report(None, None)),
        }
    }

    /// Takes one escalation step of the ladder.
    fn step(&mut self, step: Step) {
        match step {
            #[cfg(unix)]
            Step::Terminate => self.tree.terminate(),
            Step::Kill => {
                self.kill_tree();
                // The agent's own pid too, in case it left its group.
                drop(self.child.kill());
            }
        }
    }

    /// Kills what is left of the tree once the agent has exited, and reports how it exited.
    fn finish(&mut self, status: &io::Result<ExitStatus>) -> ExitReport {
        self.kill_tree();
        match status {
            Ok(status) => self.report(status.code(), signal(*status)),
            // Waiting on a child this process started does not fail in practice.
            Err(_) => self.report(None, None),
        }
    }

    fn report(&self, code: Option<i32>, signal: Option<i32>) -> ExitReport {
        ExitReport {
            code,
            signal,
            stderr: self.stderr.clone(),
        }
    }

    fn kill_tree(&mut self) {
        if !self.tree_killed {
            self.tree.kill();
            self.tree_killed = true;
        }
    }
}

impl Drop for Handle {
    fn drop(&mut self) {
        self.kill_tree();
    }
}

/// An escalation step of the ladder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Step {
    /// SIGTERM to the group.
    #[cfg(unix)]
    Terminate,
    /// SIGKILL to the group and the agent, or end the job.
    Kill,
}

/// Where the ladder ended, with the exit status when one came.
#[derive(Debug)]
enum Ended<S> {
    StdinClosed(S),
    #[cfg(unix)]
    Terminated(S),
    Killed(S),
    Unresponsive,
}

/// The ladder's waits, once stdin is closed: `exit` against each grace period, with `act`
/// taking the step that follows a wait that ran out.
async fn ladder<S, T: Timer>(
    mut exit: Pin<&mut impl Future<Output = S>>,
    timer: &T,
    mut act: impl FnMut(Step),
) -> Ended<S> {
    if let Some(status) = within(exit.as_mut(), timer.after(STDIN_GRACE)).await {
        return Ended::StdinClosed(status);
    }
    #[cfg(unix)]
    {
        act(Step::Terminate);
        if let Some(status) = within(exit.as_mut(), timer.after(TERM_GRACE)).await {
            return Ended::Terminated(status);
        }
    }
    act(Step::Kill);
    match within(exit, timer.after(KILL_GRACE)).await {
        Some(status) => Ended::Killed(status),
        None => Ended::Unresponsive,
    }
}

/// `exit`'s output if it ends before `deadline`.
async fn within<F: Future + Unpin>(
    exit: F,
    deadline: impl Future<Output = ()>,
) -> Option<F::Output> {
    match select(exit, pin!(deadline)).await {
        Either::Left((output, _)) => Some(output),
        Either::Right(((), _)) => None,
    }
}

/// The signal that ended the process.
#[cfg(unix)]
fn signal(status: ExitStatus) -> Option<i32> {
    std::os::unix::process::ExitStatusExt::signal(&status)
}

/// Windows has no signals.
#[cfg(windows)]
fn signal(_status: ExitStatus) -> Option<i32> {
    None
}

/// The stderr an exit report reads its tail from.
#[derive(Clone)]
struct StderrSource {
    ring: SharedRing,
    redactor: Arc<Redactor>,
}

impl std::fmt::Debug for StderrSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StderrSource").finish_non_exhaustive()
    }
}

/// How an agent exited.
#[derive(Debug, Clone)]
pub struct ExitReport {
    code: Option<i32>,
    signal: Option<i32>,
    stderr: StderrSource,
}

impl ExitReport {
    /// The exit code; `None` when a signal ended the agent.
    pub fn code(&self) -> Option<i32> {
        self.code
    }

    /// The signal that ended the agent (Unix); always `None` on Windows.
    pub fn signal(&self) -> Option<i32> {
        self.signal
    }

    /// The end of the agent's stderr, at most 4096 bytes, redacted, opening on a whole line: the
    /// home folder becomes `~`, and the values of the spec's secret variables (names holding
    /// `KEY`, `TOKEN`, `SECRET` or `PASSWORD`, values of 8 bytes or more), words starting `sk-`
    /// and the word after `Bearer ` (in any case) become `[redacted]`. It reads the ring when
    /// called, so drive the stderr pump to its end first to include the last lines.
    pub fn stderr_tail(&self) -> String {
        stderr::tail(&self.stderr.ring, &self.stderr.redactor)
    }

    /// The reason `host` records when the agent exits on its own.
    pub fn detach_reason(&self) -> DetachReason {
        DetachReason::Exited {
            code: self.code,
            stderr_tail: self.stderr_tail(),
        }
    }
}

/// The step of the shutdown ladder that ended the agent. Not `#[non_exhaustive]`: the ladder's
/// steps are fixed, and a new one should make every match on it fail to build.
#[derive(Debug, Clone)]
pub enum ShutdownOutcome {
    /// It exited within 2 s of its stdin closing.
    StdinClosed(ExitReport),
    /// It exited within 3 s of SIGTERM (Unix only).
    Terminated(ExitReport),
    /// It was killed: SIGKILL on Unix, its job ended on Windows.
    Killed(ExitReport),
    /// It did not exit within 2 s of its kill, so the OS still holds it (uninterruptible I/O,
    /// such as a dead network share). The report has no code and no signal. Its tree was sent
    /// its kill; it is reaped in the background once it does exit.
    Unresponsive(ExitReport),
}

impl ShutdownOutcome {
    /// How the agent exited, whichever step ended it.
    pub fn report(&self) -> &ExitReport {
        match self {
            ShutdownOutcome::StdinClosed(report)
            | ShutdownOutcome::Terminated(report)
            | ShutdownOutcome::Killed(report)
            | ShutdownOutcome::Unresponsive(report) => report,
        }
    }
}

#[cfg(test)]
mod tests;
