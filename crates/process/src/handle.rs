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
use crate::descendants::{Descendants, Snapshot};
#[cfg(unix)]
use crate::descendants::{Proc, kill_all};
use crate::live::Live;
use crate::os::{Root, Tree};
use crate::stderr::{self, Redactor, SharedRing};
use crate::stdin::StdinSlot;

/// How long shutdown waits after closing stdin, unless the spawn says otherwise
/// (`SpawnOptions::stdin_grace`). ACP agents exit when stdin ends.
pub(crate) const STDIN_GRACE: Duration = Duration::from_secs(2);
/// How long shutdown waits after SIGTERM, before SIGKILL.
#[cfg(unix)]
const TERM_GRACE: Duration = Duration::from_secs(3);
/// How long shutdown waits for the exit after the kill, before it reports the agent unresponsive.
const KILL_GRACE: Duration = Duration::from_secs(2);
/// How often [`Handle::exited_draining`] asks whether the tree is gone once the agent exited.
#[cfg(windows)]
const GONE_POLL: Duration = Duration::from_millis(20);
/// How long [`Handle::exited_draining`] still reads, once only spared browsers are left, for
/// what the tree wrote before it ended: every read returns at once then.
#[cfg(windows)]
const DRAIN: Duration = Duration::from_millis(200);

/// An agent's process and its tree. Dropping it kills the tree at once, without waiting.
#[derive(Debug)]
pub struct Handle {
    child: Child,
    tree: Tree,
    root: Root,
    /// The descendants the shutdown was given (Unix), killed after the group: a command in a
    /// session of its own has left the group, and once the agent is gone it is no longer a
    /// descendant to find.
    #[cfg(unix)]
    listed: Option<Vec<Proc>>,
    stdin: StdinSlot,
    stderr: StderrSource,
    /// Whether the tree has been sent its kill, so `Drop` sends none: on Unix a group id can be
    /// reused once the group is gone.
    tree_killed: bool,
    /// How long the shutdown waits after closing stdin (`SpawnOptions::stdin_grace`).
    stdin_grace: Duration,
    /// The start's count (`live.rs`), given back when the handle drops, after the tree's kill.
    _live: Live,
}

impl Handle {
    pub(crate) fn new(
        child: Child,
        tree: Tree,
        root: Root,
        stdin: StdinSlot,
        ring: SharedRing,
        redactor: Arc<Redactor>,
        stdin_grace: Duration,
        live: Live,
    ) -> Handle {
        Handle {
            child,
            tree,
            root,
            #[cfg(unix)]
            listed: None,
            stdin,
            stderr: StderrSource { ring, redactor },
            tree_killed: false,
            stdin_grace,
            _live: live,
        }
    }

    /// The agent's process id.
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// A [`TreeWatch`] on the agent's tree, which outlives the handle.
    pub fn tree_watch(&self) -> TreeWatch {
        TreeWatch(crate::os::watch(&self.tree))
    }

    /// A [`TreeKill`] on the agent's tree, which outlives the handle (Unix).
    #[cfg(unix)]
    pub fn tree_kill(&self) -> TreeKill {
        TreeKill {
            group: crate::os::watch(&self.tree),
            root: self.root.clone(),
        }
    }

    /// The agent's descendants, to list and end while it runs (see [`Descendants`]).
    pub fn descendants(&self) -> Descendants {
        Descendants::new(self.root.clone())
    }

    /// Waits for the agent to exit on its own, then kills what is left of its tree, so a helper
    /// it started cannot outlive it. Dropping the future and calling again is fine.
    pub async fn exited(&mut self) -> ExitReport {
        let status = self.child.status().await;
        self.finish(&status)
    }

    /// [`Handle::exited`], with `pipes` (the reads of the agent's stdout and stderr to their
    /// end) driven beside it until both are done. Returns the report and whether the pipes
    /// reached their end.
    ///
    /// On Windows a browser the tree's kill spared (`LaunchSpec::spare_browsers`) can hold those
    /// pipes open for as long as it runs: it inherits every inheritable handle of the process
    /// that starts it (Codex's `webbrowser` starts it by `CreateProcessW` with inheritance on),
    /// the agent's stdout and stderr among them. So once the agent has exited and the rest of
    /// its tree is gone, the spared browsers and their children aside ([`TreeWatch::gone`]),
    /// the reads get 200 ms more for what is already in the pipes, then are dropped.
    /// Elsewhere, and while any other process of the tree runs, it waits for the pipes' end.
    pub async fn exited_draining<T: Timer>(
        &mut self,
        timer: &T,
        pipes: impl Future<Output = ()>,
    ) -> (ExitReport, bool) {
        let mut pipes = pin!(pipes);
        let report = {
            let exited = pin!(self.exited());
            match select(pipes.as_mut(), exited).await {
                Either::Left(((), exited)) => return (exited.await, true),
                Either::Right((report, _)) => report,
            }
        };
        let watch = self.tree_watch();
        let settled = pin!(settled(&watch, timer));
        let ended = matches!(select(pipes, settled).await, Either::Left(_));
        (report, ended)
    }

    /// Stops the agent by the ladder: close its stdin and wait the stdin grace (2 s unless
    /// `SpawnOptions::stdin_grace` set another), then (Unix) SIGTERM to its group and wait 3 s,
    /// then SIGKILL to the group, or on Windows end its job, and wait 2 s more for the exit.
    /// Whichever way it exits, what is left of its tree is killed: on Unix the group, then every
    /// process of `listed` that still runs with its listed start (a command in a session of its
    /// own has left the group). Take `listed` from [`Handle::descendants`] just before, where a
    /// blocking listing may run; `None` (a listing that failed or was not taken) kills the group
    /// only. Windows ends the whole job and does not read it. The outcome names the step that
    /// ended it, or says the agent never exited.
    ///
    /// Keep reading the agent's stdout, or drop it, while this runs: an agent blocked writing to
    /// a full stdout pipe never sees its stdin close, and costs the whole grace and the SIGTERM.
    pub async fn shutdown<T: Timer>(
        mut self,
        timer: &T,
        listed: Option<Snapshot>,
    ) -> ShutdownOutcome {
        #[cfg(unix)]
        {
            self.listed = listed.and_then(Snapshot::into_processes);
        }
        #[cfg(windows)]
        drop(listed);
        self.stdin.close();
        let exit = pin!(self.child.status());
        let ended = ladder(exit, timer, self.stdin_grace, |step| self.step(step)).await;
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
                let tree_ended = self.kill_tree();
                // The agent's own pid too: on Unix in case it left its group; on Windows only
                // when the job would not end, so the agent reports the job's exit code.
                if cfg!(unix) || !tree_ended {
                    drop(self.child.kill());
                }
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

    /// Sends the tree its kill, once. Returns whether the OS took it (an empty Unix group counts),
    /// or true when it was sent before.
    fn kill_tree(&mut self) -> bool {
        if self.tree_killed {
            return true;
        }
        self.tree_killed = true;
        let ended = self.tree.kill();
        #[cfg(unix)]
        if let Some(listed) = self.listed.take() {
            kill_all(&listed);
        }
        ended
    }
}

impl Drop for Handle {
    /// Kills the group or the job, without listing (a listing blocks): a descendant outside the
    /// group outlives a handle dropped without a shutdown. A Windows tree that spares a browser
    /// (`LaunchSpec::spare_browsers`) is the exception: its kill looks up the default browser in
    /// the registry, lists the job and waits up to 1.5 s for the processes it ends.
    fn drop(&mut self) {
        self.kill_tree();
    }
}

/// Asks whether an agent's tree is gone, from any thread, after its [`Handle`] has dropped:
/// on Unix its process group is empty, the agent reaped included; on Windows its Job Object
/// holds no running process. It sends nothing; the tree's kill is the handle's.
///
/// On Windows it holds the job open, so the job's kill-on-close waits for it too; the handle
/// has always sent the job its `TerminateJobObject` by the time it drops.
#[derive(Debug, Clone)]
pub struct TreeWatch(crate::os::Watch);

impl TreeWatch {
    /// Whether the tree is gone. A blocking-free OS query; poll it.
    pub fn gone(&self) -> bool {
        self.0.gone()
    }
}

/// Ends an agent's tree from any thread, as the handle's kill does after a stop: it lists the
/// agent's descendants, sends SIGKILL to the group, then to each listed process still running
/// with its listed start (a command in a session of its own has left the group). For a watcher
/// that must end a session at once without the handle: the macOS git watch (decision record
/// 0038). The [`Handle`] still reports the exit. Unix only.
#[cfg(unix)]
#[derive(Debug, Clone)]
pub struct TreeKill {
    group: crate::os::Watch,
    root: Root,
}

#[cfg(unix)]
impl TreeKill {
    /// Sends the kill; blocking for the listing (see [`Descendants::snapshot`]). A group
    /// already empty gets no signal, and nothing is listed once the agent is gone. Returns
    /// whether the group's kill went or the group was gone.
    pub fn kill(&self) -> bool {
        let listed = self.root.list();
        let ended = self.group.gone() || self.group.kill();
        if let Some(listed) = listed {
            kill_all(&listed);
        }
        ended
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

/// The ladder's waits, once stdin is closed: `exit` against each grace period (`stdin_grace` for
/// the first), with `act` taking the step that follows a wait that ran out.
async fn ladder<S, T: Timer>(
    mut exit: Pin<&mut impl Future<Output = S>>,
    timer: &T,
    stdin_grace: Duration,
    mut act: impl FnMut(Step),
) -> Ended<S> {
    if let Some(status) = within(exit.as_mut(), timer.after(stdin_grace)).await {
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

/// Ends once the tree is gone, spared browsers and their children aside, and [`DRAIN`] more
/// has passed: asked every [`GONE_POLL`].
#[cfg(windows)]
async fn settled<T: Timer>(watch: &TreeWatch, timer: &T) {
    while !watch.gone() {
        timer.after(GONE_POLL).await;
    }
    timer.after(DRAIN).await;
}

/// Never ends: no Unix stop spares a process, so the pipes' end is awaited as it always was.
#[cfg(unix)]
async fn settled<T: Timer>(_watch: &TreeWatch, _timer: &T) {
    futures::future::pending::<()>().await;
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

    /// The end of the agent's stderr, at most 4096 bytes, redacted, opening on a whole line. The
    /// home folder becomes `~`. The values of the spec's secret variables (names holding `KEY`,
    /// `TOKEN`, `SECRET` or `PASSWORD`, values of 8 bytes or more), words starting `sk-` and
    /// the word after `Bearer ` (in any case) become `[redacted]`. A secret that starts before
    /// the part of stderr it redacts is cut there, and the end that is left is left out too, so
    /// no part of a secret is at the start.
    ///
    /// It reads the ring when called, so drive the stderr pump to its end first to include the
    /// last lines.
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
    /// It exited within the stdin grace (2 s unless `SpawnOptions::stdin_grace` set another) of
    /// its stdin closing.
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
