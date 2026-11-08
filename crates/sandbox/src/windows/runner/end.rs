//! The session's end (M1), apart from the outer's calls so the tests hold it on every OS.
//!
//! The outer ends every session once: when the inner runner exited, once the agent's output had
//! its time ([`inner_exited`]); when the host's stdin ended (the host's cancel ladder closes it,
//! then gives the outer its `stdin_grace`, 5 s on tier B, before it ends the outer's job), once
//! the agent, whose stdin is closed first, had [`GRACE`] to finish and the inner didn't exit in it
//! ([`relay_stdin`]); or at once on any failure once the launch is read.
//! [`SessionEnd::trigger`] ends it once a session, whichever path comes first; a path that comes
//! while the end is still going waits for it.
//!
//! First the launch's kill-on-close job is ended and the outer waits, up to [`EMPTY_WAIT`], until
//! no process is left in it. The session can launch nothing more from here. It reads
//! `SweepNext` (`M`), then asks the Task Scheduler to run the `SYSTEM` task
//! `\Clauseo\SandboxSweep` ([`EndSteps::request_sweep`]), which deletes the pool users' scheduled
//! tasks and cancels their BITS jobs, and kills nothing; it does so even when the job didn't empty,
//! which is logged. The run is queued as the outer returns from the request, so the host ending
//! the outer later doesn't stop it, and the run takes `M` or a later number. The outer starts no
//! process of the pool user's at the end: a process of the pool user's outside the job is killed
//! by the next inner's first act, and its per-user COM classes, `Run` and `RunOnce` values and
//! Startup entries are removed there too (`inner_flow.rs`). A request that fails is logged and
//! changes no exit code.
//!
//! The outer never waits for the run: the host gives it 5 s after a cancel (`stdin_grace` on
//! tier B), and its end fits in 3 s. At its end ([`SessionEnd::finish`]) it writes the session's
//! marker as awaiting a sweep `>= M`, whatever the end was (a failure, a cancel past its grace or
//! a job that didn't empty too: the next inner's first act kills what is left and reports an
//! escapee). When a sweep found an escapee it reads `StopNext`, requests `\Clauseo\SandboxStop`
//! and writes the marker as awaiting that stop alone: its `M` was read while the escapee lived,
//! so a sweep reading it could have completed before the escapee registered a task. Once the
//! stop completed, whoever reads the marker reads `SweepNext` then (`unclean.rs`). The next launch
//! (`clean_close::check_launch`) and the pool user's next logon (`outer/unclean.rs`) check it
//! (`marker.rs`).
//!
//! The guard keeps the signal that began the end, so a cancel exits as it would have without the
//! job's end ([`exit_code`]). It also keeps whether any sweep of the session found an escapee (the
//! inner's, reported on `ctl`), which makes the outer exit `RunExit::Escapee` whatever the agent's
//! code ([`SessionEnd::outcome`]).
//!
//! The git watch ends a session with [`SessionEnd::kill_now`] instead: the job ended and empty
//! is all it waits for before it moves anything, and the sweep task's request follows on a thread
//! of its own, once as ever. The end is known as begun before the job's end, so an inner that the
//! outer is still starting is never resumed past it ([`SessionEnd::began`], `start.rs`).

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use super::marker::MarkerState;
use super::sweep::SWEPT;
use super::{RunExit, agent_code};
use crate::windows::setup::record_values::{STOP_NEXT, SWEEP_NEXT};

/// The exit code the host's own job end gives the outer, `TERMINATED` in
/// `crates/process/src/os/windows.rs:54` (private there): what a cancel whose agent didn't exit
/// within the host's `stdin_grace` gives without the outer's own job end.
pub(crate) const HOST_ENDED: u32 = 0xC1A0;
/// How long the agent has, once the host's stdin ended and the agent's was closed, to finish
/// before the outer ends the job. The host gives the outer 5 s once it closed the outer's stdin
/// (`SpawnOptions::stdin_grace` on tier B), before it ends the outer's job; the outer's end, this
/// grace, the job's end, the reads, the request and the marker, fits in 3 s (`OUTER_END`).
pub(crate) const GRACE: Duration = Duration::from_millis(1000);
/// What the outer's end after a cancel fits in, from the host's stdin end: well inside the host's
/// 5 s. The cancel tests pin it.
#[cfg(test)]
pub(crate) const OUTER_END: Duration = Duration::from_secs(3);
/// How long the outer waits for its ended job to empty before it requests the sweep task.
pub(crate) const EMPTY_WAIT: Duration = Duration::from_millis(400);
/// How often it looks meanwhile.
pub(crate) const EMPTY_POLL: Duration = Duration::from_millis(25);
/// How many pieces of the host's stdin (each up to [`PIECE`]) wait for the agent's stdin: up to
/// 16 MiB the agent hasn't read yet, past which the read waits too ([`relay_stdin`]).
const STDIN_PIECES: usize = 256;
/// The largest piece read from the host's stdin at once.
const PIECE: usize = 64 * 1024;

/// The calls that end a session; Windows' are `outer/end.rs`'s.
pub(crate) trait EndSteps {
    /// Ends this launch's kill-on-close job (its processes end with [`SWEPT`]) and waits, up to
    /// [`EMPTY_WAIT`], until no process is left in it ([`wait_empty`]); nothing to do before the
    /// job is made. An error says what is left.
    fn empty_job(&self) -> Result<(), String>;
    /// The record's `REG_QWORD` value `name` (`SweepNext`, `StopNext`) as the lawyer reads it,
    /// 1 when absent: the number the task's next run takes. Read without the set-up lock, which
    /// only an administrator can take, once the session can launch nothing more
    /// (`pool_steps::sequence` says why that is sound).
    fn next_number(&self, name: &'static str) -> Result<u64, String>;
    /// Asks the Task Scheduler to run `\Clauseo\SandboxSweep` now, and returns once the run is
    /// queued. An error names the failed call and its code, never a path or a SID.
    fn request_sweep(&self) -> Result<(), String>;
    /// Asks the Task Scheduler to run `\Clauseo\SandboxStop` now: whether the run was queued.
    fn request_stop(&self) -> bool;
    /// Writes the session's marker as `state`, in place of the open one.
    fn settle(&self, state: &MarkerState) -> Result<(), String>;
    /// A line on the outer's stderr.
    fn log(&self, line: &str);
    /// The time on a monotonic clock, from any fixed origin: how long ago the host's stdin ended.
    fn now(&self) -> Duration;
}

/// Ends the session's job, reads `SweepNext`, then requests the sweep task, even when the job
/// didn't empty; each failure is logged as a warning. Whether the job emptied, and the
/// `SweepNext` read (`None` when it couldn't be).
pub(crate) fn end_session<S: EndSteps>(steps: &S) -> (bool, Option<u64>) {
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
    // The session can launch nothing more now. Read before the request, so the run it queues
    // takes this number or a later one.
    let sweep = next_number(steps, SWEEP_NEXT);
    if let Err(what) = steps.request_sweep() {
        steps.log(&format!(
            "clauseo-sandbox-run: warning: the sandbox sweep task couldn't be requested \
             ({what})"
        ));
    }
    (emptied, sweep)
}

/// [`EndSteps::next_number`], a failure logged: `None`, and the marker stays open.
fn next_number<S: EndSteps>(steps: &S, name: &'static str) -> Option<u64> {
    steps
        .next_number(name)
        .inspect_err(|what| {
            steps.log(&format!(
                "clauseo-sandbox-run: warning: {name} can't be read ({what}); the session marker \
                 stays open"
            ));
        })
        .ok()
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

/// The guard that ends the session once.
pub(crate) struct SessionEnd<S: EndSteps> {
    steps: S,
    /// The `SweepNext` read once a path's end of the job and request of the sweep task is over
    /// (`None` when it couldn't be read).
    started: OnceLock<Option<u64>>,
    /// Whether the marker was settled ([`SessionEnd::finish`]).
    finished: OnceLock<()>,
    /// When the host's stdin ended, on [`EndSteps::now`]'s clock.
    stdin_ended: OnceLock<Duration>,
    /// Whether a sweep of the session found a process of the pool user's it couldn't end.
    escaped: AtomicBool,
    /// The end signal of the path that began the end, set before that path ends the job.
    begun: Mutex<Option<EndSignal>>,
    begun_changed: Condvar,
}

impl<S: EndSteps> SessionEnd<S> {
    pub(crate) fn new(steps: S) -> SessionEnd<S> {
        SessionEnd {
            steps,
            started: OnceLock::new(),
            finished: OnceLock::new(),
            stdin_ended: OnceLock::new(),
            escaped: AtomicBool::new(false),
            begun: Mutex::new(None),
            begun_changed: Condvar::new(),
        }
    }

    /// Ends the job and requests the sweep task ([`end_session`]) unless a path did already;
    /// whether this call began the end. A call while another path's end is still going returns
    /// once that end is over, so no path goes on (the outer's exit among them) with the job still
    /// being ended or the request still going.
    pub(crate) fn trigger(&self, why: EndSignal) -> bool {
        let first = self.begin(why);
        self.start_once();
        first
    }

    /// The git watch's end: begins the end (`why`), ends the job and waits, up to
    /// [`EMPTY_WAIT`], until no process is left in it ([`EndSteps::empty_job`]), whichever
    /// path began the end. Whether it is empty: only then may the watch move anything. The
    /// sweep task's request comes after, on a thread of its own ([`SessionEnd::trigger`]'s end,
    /// once a session; this thread's when none starts), so it never holds up the moves; a path
    /// that ends the session meanwhile waits for it as ever.
    pub(crate) fn kill_now(self: &Arc<Self>, why: EndSignal) -> bool
    where
        S: Send + Sync + 'static,
    {
        self.begin(why);
        let emptied = self.steps.empty_job();
        if let Err(what) = &emptied {
            self.steps.log(&format!(
                "clauseo-sandbox-run: the session's job didn't empty: {what}"
            ));
        }
        let end = Arc::clone(self);
        let later = thread::Builder::new()
            .name("session-end".to_owned())
            .spawn(move || end.start_once());
        if later.is_err() {
            self.start_once();
        }
        emptied.is_ok()
    }

    /// Begins the end with `why` unless a path did already, before anything ends the job;
    /// whether this call did.
    fn begin(&self, why: EndSignal) -> bool {
        let mut begun = self.begun.lock().unwrap_or_else(PoisonError::into_inner);
        let first = begun.is_none();
        if first {
            *begun = Some(why);
        }
        drop(begun);
        self.begun_changed.notify_all();
        first
    }

    /// Ends the job and requests the sweep task, once a session ([`end_session`]); a call
    /// while another's is going waits for it.
    fn start_once(&self) {
        self.started.get_or_init(|| end_session(&self.steps).1);
    }

    /// The host's stdin ended now: from here the host gives the outer its `stdin_grace`, and the
    /// agent what is left of its grace. The first call counts.
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
            .wait_timeout_while(begun, wait, |begun| begun.is_none())
            .is_ok_and(|(begun, _)| begun.is_some())
    }

    /// The end signal of the path that began the end, once one did. It is set, under the lock,
    /// before that path ends the job, so a step done before a read of `None` (the inner put in
    /// the job) comes before the job's end, which then ends what the step left there. The outer
    /// reads it after it put the inner in the job and before it resumes it (`start.rs`).
    pub(crate) fn began(&self) -> Option<EndSignal> {
        *self.begun.lock().unwrap_or_else(PoisonError::into_inner)
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

    /// At the outer's end, once: the session's marker settled (`marker.rs`); it never waits for a
    /// run. With no end begun it stays open. With no escapee it awaits a sweep `>= M`, the
    /// `SweepNext` read at the end: whatever the end was, since the next inner's first act kills
    /// what is left of the pool user's and reports an escapee. When a sweep found an escapee,
    /// `StopNext` is read, the stop task requested, and it awaits that stop alone (its sweep is
    /// read once the stop completed, `unclean.rs`). A number that couldn't be read leaves it
    /// open, which the pool user's next logon, or the desktop's start, makes one awaiting a stop
    /// (`outer/unclean.rs`).
    pub(crate) fn finish(&self) {
        self.finished.get_or_init(|| {
            let Some(sweep) = self.started.get() else {
                return;
            };
            let state = if self.escaped() {
                self.awaiting_stop()
            } else {
                sweep.map(|sweep| MarkerState::AwaitingSweep { sweep })
            };
            let Some(state) = state else {
                return;
            };
            if let Err(what) = self.steps.settle(&state) {
                self.steps.log(&format!(
                    "clauseo-sandbox-run: warning: the session marker couldn't be written \
                     ({what}); it stays open"
                ));
            }
        });
    }

    /// An escapee's marker: `StopNext` read before the stop task is requested, so the run it
    /// queues takes that number or a later one; `None` when it couldn't be read. No sweep number:
    /// one read while the escapee lives proves nothing.
    fn awaiting_stop(&self) -> Option<MarkerState> {
        let stop = next_number(&self.steps, STOP_NEXT);
        if !self.steps.request_stop() {
            self.steps
                .log("clauseo-sandbox-run: warning: the sandbox stop task couldn't be requested");
        }
        Some(MarkerState::AwaitingStop { stop: stop? })
    }

    /// The outer's exit code for a run that would exit `code`: [`RunExit::Escapee`] when a sweep
    /// of the session found an escapee, whatever `code` is (the agent's own, even 0, which the
    /// outer then says on stderr); else `code`. The sweep and stop tasks never change it.
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

/// The outer's exit code for a session whose end began (`began`) before its inner ran: the code
/// the job's end would have given a running agent ([`exit_code`] of [`SWEPT`], none reported).
/// No agent runs.
pub(crate) fn ended_before_start(began: EndSignal) -> u32 {
    exit_code(Some(began), SWEPT, None)
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
/// with it, inside the host's `stdin_grace`; a writer still blocked then fails, its pipe's other
/// end gone with the job, and closes. An agent that closed its own stdin ends nothing: the writer
/// stops, and the host's stdin is still read, its end still kept and acted on. Past
/// [`STDIN_PIECES`] pieces the agent hasn't read, the read waits for the writer, so the end is
/// seen late.
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
mod kill_tests;
#[cfg(test)]
mod stdin_tests;
#[cfg(test)]
mod tests;
