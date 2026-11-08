//! The session's end (M1), apart from the outer's calls so the tests hold it on every OS.
//!
//! The outer ends every session once: when the inner runner exited, once the agent's output had
//! its time ([`inner_exited`]); when the host's stdin ended (the host's cancel ladder closes it,
//! then gives the outer 2 s before it ends the outer's job), once the agent, whose stdin is closed
//! first, had [`GRACE`] to finish and the inner didn't exit in it ([`relay_stdin`]); or at once
//! on any failure once the launch is read. [`SessionEnd::trigger`] ends it once a session,
//! whichever path comes first; a path that comes while the end is still going waits for it.
//!
//! First the launch's kill-on-close job is ended and the outer waits, up to [`EMPTY_WAIT`], until
//! no process is left in it. Then it asks the Task Scheduler to run the `SYSTEM` task
//! `\Clauseo\SandboxSweep` ([`EndSteps::request_sweep`]), which deletes the pool users' scheduled
//! tasks and cancels their BITS jobs, and kills nothing; it does so even when the job didn't empty,
//! which is logged and leaves the session unclean. The run is queued as the outer returns from the
//! request, so the host ending the outer later doesn't stop it. The outer starts no process of the
//! pool user's at the end: a process of the pool user's outside the job is killed by the next
//! inner's first act, and its per-user COM classes, `Run` and `RunOnce` values and Startup entries
//! are removed there too (`inner_flow.rs`). A request that fails is logged and changes no exit
//! code. At its own end, if it lives that long, the outer waits up to [`SWEEP_WAIT`] for the run's
//! result ([`SessionEnd::wait`]): only 0 is a clean sweep.
//!
//! The guard keeps the signal that began the end, so a cancel exits as it would have without the
//! job's end ([`exit_code`]). It also keeps whether any sweep of the session found an escapee (the
//! inner's, reported on `ctl`), which makes the outer exit `RunExit::Escapee` whatever the agent's
//! code ([`SessionEnd::outcome`]).

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, OnceLock, mpsc};
use std::thread;
use std::time::Duration;

use super::sweep::SWEPT;
use super::{RunExit, agent_code};

/// The exit code the host's own job end gives the outer, `TERMINATED` in
/// `crates/process/src/os/windows.rs:54` (private there): what a cancel whose agent didn't exit
/// within the host's 2 s gives without the outer's own job end.
pub(crate) const HOST_ENDED: u32 = 0xC1A0;
/// How long the agent has, once the host's stdin ended and the agent's was closed, to finish
/// before the outer ends the job: half of the 2 s the host gives the outer once it closed the
/// outer's stdin, before it ends the outer's job (`STDIN_GRACE` in `crates/process/src/handle.rs`,
/// private there), so the job's end and the sweep task's request fit in the rest.
pub(crate) const GRACE: Duration = Duration::from_millis(1000);
/// How long the outer waits for its ended job to empty before it requests the sweep task.
pub(crate) const EMPTY_WAIT: Duration = Duration::from_millis(400);
/// How often it looks meanwhile.
pub(crate) const EMPTY_POLL: Duration = Duration::from_millis(25);
/// How long the outer, still alive at its end, waits for the sweep task's result.
pub(crate) const SWEEP_WAIT: Duration = Duration::from_secs(20);
/// How often it reads the result meanwhile.
pub(crate) const SWEEP_POLL: Duration = Duration::from_millis(100);
/// How many pieces of the host's stdin (each up to [`PIECE`]) wait for the agent's stdin: up to
/// 16 MiB the agent hasn't read yet, past which the read waits too ([`relay_stdin`]).
const STDIN_PIECES: usize = 256;
/// The largest piece read from the host's stdin at once.
const PIECE: usize = 64 * 1024;

/// The calls that end a session; Windows' are `outer/end.rs`'s.
pub(crate) trait EndSteps {
    /// A run of the sweep task, once requested.
    type Run;
    /// Ends this launch's kill-on-close job (its processes end with [`SWEPT`]) and waits, up to
    /// [`EMPTY_WAIT`], until no process is left in it ([`wait_empty`]); nothing to do before the
    /// job is made. An error says what is left.
    fn empty_job(&self) -> Result<(), String>;
    /// Asks the Task Scheduler to run `\Clauseo\SandboxSweep` now, and returns once the run is
    /// queued. An error names the failed call and its code, never a path or a SID.
    fn request_sweep(&self) -> Result<Self::Run, String>;
    /// How `run` ended: `None` while it is queued or running, else the task's last result.
    fn sweep_result(&self, run: &Self::Run) -> Result<Option<u32>, String>;
    /// Waits `pause` before the next read of the result.
    fn pause(&self, pause: Duration);
    /// A line on the outer's stderr.
    fn log(&self, line: &str);
    /// The time on a monotonic clock, from any fixed origin: how long ago the host's stdin ended.
    fn now(&self) -> Duration;
}

/// How the session's sweep task ended, as far as the outer saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Swept {
    /// Its run's result was 0: every task and BITS job of the pool users' it found went.
    Clean,
    /// Its result wasn't 0, it was still queued or running when the outer's wait ran out, its
    /// request or its result's read failed, or no run was requested.
    Unclean,
}

/// Ends the session's job and requests the sweep task, even when the job didn't empty; each
/// failure is logged as a warning. Whether the job emptied, and the run once requested.
pub(crate) fn end_session<S: EndSteps>(steps: &S) -> (bool, Option<S::Run>) {
    let emptied = match steps.empty_job() {
        Ok(()) => true,
        Err(what) => {
            steps.log(&format!(
                "clauseo-sandbox-run: warning: the job didn't empty ({what}); the sandbox sweep \
                 task is requested all the same"
            ));
            false
        }
    };
    let run = match steps.request_sweep() {
        Ok(run) => Some(run),
        Err(what) => {
            steps.log(&format!(
                "clauseo-sandbox-run: warning: the sandbox sweep task couldn't be requested \
                 ({what})"
            ));
            None
        }
    };
    (emptied, run)
}

/// Reads `run`'s result every [`SWEEP_POLL`] (through [`EndSteps::pause`]) for up to
/// [`SWEEP_WAIT`]: [`Swept::Clean`] for 0 alone. Anything else is logged as a warning: a
/// non-zero result (in hex), a run still queued or running at the deadline, a read that failed,
/// or no run (`None`: its request failed or none was made).
pub(crate) fn wait_swept<S: EndSteps>(steps: &S, run: Option<&S::Run>) -> Swept {
    let Some(run) = run else {
        steps.log("clauseo-sandbox-run: warning: no sandbox sweep task was requested");
        return Swept::Unclean;
    };
    let polls = SWEEP_WAIT.as_millis() / SWEEP_POLL.as_millis().max(1);
    let mut polled = 0;
    loop {
        match steps.sweep_result(run) {
            Ok(Some(0)) => return Swept::Clean,
            Ok(Some(code)) => {
                steps.log(&format!(
                    "clauseo-sandbox-run: warning: the sandbox sweep task ended 0x{code:08X}"
                ));
                return Swept::Unclean;
            }
            Ok(None) if polled < polls => {
                steps.pause(SWEEP_POLL);
                polled += 1;
            }
            Ok(None) => {
                steps.log(&format!(
                    "clauseo-sandbox-run: warning: the sandbox sweep task was still running after \
                     {} s",
                    SWEEP_WAIT.as_secs()
                ));
                return Swept::Unclean;
            }
            Err(what) => {
                steps.log(&format!(
                    "clauseo-sandbox-run: warning: the sandbox sweep task's result can't be read \
                     ({what})"
                ));
                return Swept::Unclean;
            }
        }
    }
}

/// Asks `active` for the job's process count until it is 0, every `poll` (through `pause`)
/// for up to `wait`: [`EndSteps::empty_job`]'s wait once the job is ended. An error names
/// the count left, or the failed call's code.
pub(crate) fn wait_empty(
    mut active: impl FnMut() -> io::Result<u32>,
    mut pause: impl FnMut(Duration),
    wait: Duration,
    poll: Duration,
) -> Result<(), String> {
    let pauses = wait.as_millis() / poll.as_millis().max(1);
    let mut paused = 0;
    loop {
        let left = active().map_err(|error| {
            let code = error
                .raw_os_error()
                .map_or_else(|| error.kind().to_string(), |code| code.to_string());
            format!("the job's process count failed ({code})")
        })?;
        if left == 0 {
            return Ok(());
        }
        if paused >= pauses {
            return Err(format!(
                "{left} processes were still in the job after {} ms",
                wait.as_millis()
            ));
        }
        pause(poll);
        paused += 1;
    }
}

/// Why the session is ending, for the log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EndSignal {
    /// The host's stdin ended.
    Stdin,
    /// The inner runner exited.
    InnerExited,
    /// The outer failed.
    Failed,
}

/// How the session's end began: the signal, whether the job emptied, and the sweep task's run
/// once requested.
struct Began<R> {
    /// The signal.
    why: EndSignal,
    /// Whether the job emptied within [`EMPTY_WAIT`].
    emptied: bool,
    /// The sweep task's run; `None` when its request failed.
    run: Option<R>,
}

/// The guard that ends the session once.
pub(crate) struct SessionEnd<S: EndSteps> {
    steps: S,
    /// How the end began, once a path began it.
    began: OnceLock<Began<S::Run>>,
    /// How the sweep task ended, once the outer waited for it.
    swept: OnceLock<Swept>,
    /// When the host's stdin ended, on [`EndSteps::now`]'s clock.
    stdin_ended: OnceLock<Duration>,
    /// Whether a sweep of the session found a process of the pool user's it couldn't end.
    escaped: AtomicBool,
    /// Whether a path began the end, for [`SessionEnd::wait_begun`].
    begun: Mutex<bool>,
    begun_changed: Condvar,
}

impl<S: EndSteps> SessionEnd<S> {
    pub(crate) fn new(steps: S) -> SessionEnd<S> {
        SessionEnd {
            steps,
            began: OnceLock::new(),
            swept: OnceLock::new(),
            stdin_ended: OnceLock::new(),
            escaped: AtomicBool::new(false),
            begun: Mutex::new(false),
            begun_changed: Condvar::new(),
        }
    }

    /// Ends the job and requests the sweep task ([`end_session`]) unless a path did already;
    /// whether this call did. A call while another path's end is still going returns once that
    /// end is over, so no path goes on (the outer's exit among them) with the job still being
    /// ended or the request still going.
    pub(crate) fn trigger(&self, why: EndSignal) -> bool {
        let mut first = false;
        self.began.get_or_init(|| {
            first = true;
            if let Ok(mut begun) = self.begun.lock() {
                *begun = true;
            }
            self.begun_changed.notify_all();
            let (emptied, run) = end_session(&self.steps);
            Began { why, emptied, run }
        });
        first
    }

    /// The host's stdin ended now: from here the host gives the outer 2 s, and the agent what is
    /// left of its grace. The first call counts.
    pub(crate) fn stdin_ended(&self) {
        self.stdin_ended.get_or_init(|| self.steps.now());
    }

    /// What is left of `grace` since the host's stdin ended, on the steps' clock; all of it
    /// before then.
    fn grace_left(&self, grace: Duration) -> Duration {
        self.stdin_ended.get().map_or(grace, |ended| {
            grace.saturating_sub(self.steps.now().saturating_sub(*ended))
        })
    }

    /// Waits up to `wait` for a path to begin the end; whether one did.
    pub(crate) fn wait_begun(&self, wait: Duration) -> bool {
        let Ok(begun) = self.begun.lock() else {
            return false;
        };
        self.begun_changed
            .wait_timeout_while(begun, wait, |begun| !*begun)
            .is_ok_and(|(begun, _)| *begun)
    }

    /// The end signal that began the end, once one did.
    pub(crate) fn began(&self) -> Option<EndSignal> {
        self.began.get().map(|began| began.why)
    }

    /// The steps.
    pub(crate) fn steps(&self) -> &S {
        &self.steps
    }

    /// A sweep found an escapee: the inner reported one on `ctl`.
    pub(crate) fn mark_escapee(&self) {
        self.escaped.store(true, Ordering::SeqCst);
    }

    /// Whether a sweep of the session found an escapee.
    pub(crate) fn escaped(&self) -> bool {
        self.escaped.load(Ordering::SeqCst)
    }

    /// At the outer's end: waits up to [`SWEEP_WAIT`] for the sweep task's run ([`wait_swept`]),
    /// once; a later call gives the first one's answer. [`Swept::Unclean`] when no path began
    /// the end or its request failed.
    pub(crate) fn wait(&self) -> Swept {
        *self.swept.get_or_init(|| {
            let run = self.began.get().and_then(|began| began.run.as_ref());
            wait_swept(&self.steps, run)
        })
    }

    /// Whether the session ended clean, so the outer removes its unclean-session marker
    /// (`marker.rs`): its end began at the inner's exit (no failure, and no cancel whose agent
    /// outlived [`GRACE`]: an agent that finished after the host's stdin ended, its output
    /// drained before the grace ran out, ends clean), the inner reported the agent's exit
    /// (`reported`), the job emptied, the sweep task's run ended 0 (`swept`), and no sweep found
    /// an escapee. A sweep task that couldn't read or delete a task ends non-zero, so the marker
    /// stays.
    pub(crate) fn clean(&self, swept: Swept, reported: bool) -> bool {
        self.began
            .get()
            .is_some_and(|began| began.why == EndSignal::InnerExited && began.emptied)
            && reported
            && swept == Swept::Clean
            && !self.escaped()
    }

    /// The outer's exit code for a run that would exit `code`: [`RunExit::Escapee`] when a sweep
    /// of the session found an escapee, whatever `code` is (the agent's own, even 0, which the
    /// outer then says on stderr); else `code`. The sweep task's result never changes it.
    pub(crate) fn outcome(&self, code: u32) -> u32 {
        if self.escaped() {
            RunExit::Escapee.code()
        } else {
            code
        }
    }
}

/// The outer's exit code once the inner exited with `inner`, the agent having reported `reported`
/// on `ctl` if it exited first: the agent's code, never one in the runner's own range
/// ([`agent_code`]).
///
/// On a cancel whose agent didn't finish within [`GRACE`] (the session's end began at the host's
/// stdin, [`EndSignal::Stdin`]), the outer ends its job ([`EndSteps::empty_job`]), and the inner
/// and the agent with it, with [`SWEPT`], the sweeps' own kill code, which would come
/// back as `AgentReserved`. The run gives what it would have given without the job's end: the
/// agent's own code when it exited first and the inner reported it, or else [`HOST_ENDED`], the
/// code of the host's job end that follows a cancel whose agent didn't exit. An inner that
/// exited before the job's end gives its code as ever.
pub(crate) fn exit_code(began: Option<EndSignal>, inner: u32, reported: Option<u32>) -> u32 {
    if began == Some(EndSignal::Stdin) && inner == SWEPT {
        return match reported {
            Some(code) if code != SWEPT => agent_code(code),
            _ => HOST_ENDED,
        };
    }
    agent_code(inner)
}

/// Which end of a copy ended it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CopyEnd {
    /// The source ended or failed: for the host's stdin, an end signal.
    Source,
    /// The destination failed: the agent closed its stdin, which ends nothing.
    Destination,
}

/// Copies `from` to `to` until either end closes, flushing each piece; which end closed.
pub(crate) fn copy(mut from: impl Read, mut to: impl Write) -> CopyEnd {
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let read = match from.read(&mut buffer) {
            Ok(0) | Err(_) => return CopyEnd::Source,
            Ok(read) => read,
        };
        let Some(piece) = buffer.get(..read) else {
            return CopyEnd::Source;
        };
        if to.write_all(piece).and_then(|()| to.flush()).is_err() {
            return CopyEnd::Destination;
        }
    }
}

/// The host's stdin relayed to the agent's. This thread reads the host's stdin and hands each
/// piece over a bounded channel ([`STDIN_PIECES`]) to a writer thread of its own, which writes
/// to the agent's stdin; when the host's side ends (or fails), the stdin end's time is kept
/// ([`SessionEnd::stdin_ended`]) at once, even while the writer is still blocked on an agent that
/// isn't reading, so the deadline counts from the host's real end. The writer then hands over
/// what is left and closes the agent's stdin (`close`), so the agent reads the end of its input;
/// the agent has what is left of `grace` ([`GRACE`]), counted from the stdin end, to finish:
/// the inner's exit begins the end meanwhile ([`inner_exited`]), its output and code kept. When
/// no path began it by then, this one does ([`EndSignal::Stdin`]): the job ends, and the agent
/// with it, inside the host's 2 s; a writer still blocked then fails, its pipe's other end gone
/// with the job, and closes. An agent that closed its own stdin ends nothing: the writer stops,
/// and the host's stdin is still read, its end still kept and acted on. Past [`STDIN_PIECES`]
/// pieces the agent hasn't read, the read waits for the writer, so the end is seen late.
pub(crate) fn relay_stdin<S: EndSteps, W: Write + Send>(
    mut from: impl Read,
    to: W,
    end: &SessionEnd<S>,
    grace: Duration,
    close: impl FnOnce(W) + Send,
) {
    let (pieces, handed) = mpsc::sync_channel::<Vec<u8>>(STDIN_PIECES);
    let (closed, close_seen) = mpsc::channel::<()>();
    thread::scope(|scope| {
        let writer = thread::Builder::new().spawn_scoped(scope, move || {
            let mut to = to;
            for piece in handed {
                if to.write_all(&piece).and_then(|()| to.flush()).is_err() {
                    break;
                }
            }
            close(to);
            let _sent = closed.send(());
        });
        if writer.is_err() {
            // The agent's stdin, dropped with the closure, is closed at once.
            end.steps()
                .log("clauseo-sandbox-run: the agent's stdin writer didn't start");
        }
        let mut pieces = Some(pieces);
        let mut buffer = vec![0u8; PIECE];
        loop {
            let read = match from.read(&mut buffer) {
                Ok(0) | Err(_) => break,
                Ok(read) => read,
            };
            let piece = buffer.get(..read).unwrap_or_default().to_vec();
            // Once the writer stopped, what is read is dropped, but the end is still awaited.
            if pieces.as_ref().is_some_and(|to| to.send(piece).is_err()) {
                pieces = None;
            }
        }
        end.stdin_ended();
        drop(pieces);
        // The close comes before the grace's end, unless the writer is blocked.
        let _closed = close_seen.recv_timeout(grace);
        end.wait_begun(end.grace_left(grace));
        end.trigger(EndSignal::Stdin);
    });
}

/// The outer's order once the inner exited with `inner` (`None` when its wait failed), the agent
/// having reported `reported`: the agent's output first (`drain` waits up to its time for each
/// stream), then the job's end and the sweep task's request ([`EndSignal::InnerExited`], unless
/// the stdin's grace ran out first), then `drain` again for what a descendant left in the job
/// held. The exit code ([`exit_code`]).
pub(crate) fn inner_exited<S: EndSteps>(
    end: &SessionEnd<S>,
    inner: Option<u32>,
    reported: Option<u32>,
    mut drain: impl FnMut(),
) -> u32 {
    drain();
    end.trigger(EndSignal::InnerExited);
    drain();
    inner.map_or(RunExit::Spawn.code(), |code| {
        exit_code(end.began(), code, reported)
    })
}

#[cfg(test)]
mod stdin_tests;
#[cfg(test)]
mod tests;
