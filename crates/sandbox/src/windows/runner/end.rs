//! The detached end sweeper's start (M1), apart from the outer's calls so the tests hold it on
//! every OS.
//!
//! The outer starts it at the end of every session: when the inner runner exited, once the
//! agent's output had its time ([`inner_exited`]); when the host's stdin ended (the host's cancel
//! ladder closes it, then gives the outer 2 s before it ends the outer's job), once the agent,
//! whose stdin is closed first, had [`GRACE`] to finish and the inner didn't exit in it
//! ([`relay_stdin`]); or at once on any failure once the launch is read. [`EndSweep::trigger`]
//! starts it once a session, whichever path comes first; a path that comes while the start is
//! still going waits for it. First the launch's kill-on-close job is ended and the outer waits,
//! up to [`EMPTY_WAIT`], until no process is left in it: the sweeper's logon opens a gap in which
//! its process, thread and token carry the logon's default DACL, and nothing of the agent's may be
//! alive then to open them. A job still holding a process by then starts no sweeper; the next
//! launch's first act is the backstop. Once the host's stdin ended, the host ends the outer
//! [`HOST_BUDGET`] later, so the sweeper is started only while [`SWEEPER_BUDGET`] of that is still
//! left ([`sweeper_fits`]): a host kill between its logon and its resume would leave it suspended
//! and unshut. Too late, it is skipped as a job that doesn't empty is, and the session isn't clean.
//! Then the sweeper is started (`CreateProcessWithLogonW`,
//! suspended), its process and thread are shut as the inner's are, and it is resumed: it is never
//! put in this launch's kill-on-close job, so the host ending the outer leaves it running. It
//! needs nothing from the outer once running (`--sweep`, its own timeout). The guard keeps the
//! signal that started it, so a cancel exits as it would have without the sweeper
//! ([`exit_code`]). It also keeps whether any sweep of the session found an escapee (the inner's,
//! reported on `ctl`, or the end sweeper's exit code), which makes the outer exit
//! `RunExit::Escapee` whatever the agent's code ([`EndSweep::outcome`]).
//!
//! The git watch ends a session with [`EndSweep::kill_now`] instead: the job ended and empty
//! is all it waits for before it moves anything, and the sweeper's start follows on a thread of
//! its own, once as ever. The end is known as begun before the job's end, so an inner that the
//! outer is still starting is never resumed past it ([`EndSweep::began`], `start.rs`).

use std::io::{self, Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError, mpsc};
use std::thread;
use std::time::Duration;

use super::sweep::SWEPT;
use super::{RunExit, agent_code};

/// The exit code the host's own job end gives the outer, `TERMINATED` in
/// `crates/process/src/os/windows.rs:54` (private there): what a cancel whose agent didn't exit
/// within the host's 2 s gives without the end sweeper.
pub(crate) const HOST_ENDED: u32 = 0xC1A0;
/// How long the host gives the outer once it closed the outer's stdin, before it ends the outer's
/// job: `STDIN_GRACE` in `crates/process/src/handle.rs` (private there).
pub(crate) const HOST_BUDGET: Duration = Duration::from_secs(2);
/// How long the agent has, once the host's stdin ended and the agent's was closed, to finish
/// before the outer ends the job: half of [`HOST_BUDGET`], so the job's end and the sweeper's
/// start fit in the rest.
pub(crate) const GRACE: Duration = Duration::from_millis(1000);
/// How long the outer waits for its ended job to empty before it starts the sweeper.
pub(crate) const EMPTY_WAIT: Duration = Duration::from_millis(400);
/// How much of [`HOST_BUDGET`] must be left, after the stdin end, for the sweeper's logon, shut
/// and resume to begin ([`sweeper_fits`]). The outer's own logon of the inner, up to the agent's
/// first line, took 123 to 409 ms in the harness's runs; the outer now logs the sweeper's own
/// (`outer/end.rs`), which the harness's `cancel` row records.
pub(crate) const SWEEPER_BUDGET: Duration = Duration::from_millis(500);
/// How often it looks meanwhile.
pub(crate) const EMPTY_POLL: Duration = Duration::from_millis(25);
/// How many pieces of the host's stdin (each up to [`PIECE`]) wait for the agent's stdin: up to
/// 16 MiB the agent hasn't read yet, past which the read waits too ([`relay_stdin`]).
const STDIN_PIECES: usize = 256;
/// The largest piece read from the host's stdin at once.
const PIECE: usize = 64 * 1024;

/// The calls that start the sweeper; Windows' are `outer/end.rs`'s.
pub(crate) trait SweeperSteps {
    /// The suspended sweeper.
    type Sweeper;
    /// Ends this launch's kill-on-close job (its processes end with [`SWEPT`]) and waits, up to
    /// [`EMPTY_WAIT`], until no process is left in it ([`wait_empty`]); nothing to do before the
    /// job is made. An error says what is left.
    fn empty_job(&self) -> Result<(), String>;
    /// Starts `--sweep` as the pool user, suspended.
    fn log_on(&self) -> Result<Self::Sweeper, String>;
    /// Shuts its process and first thread to the pool user.
    fn shut(&self, sweeper: &Self::Sweeper) -> Result<(), String>;
    /// Lets it run.
    fn resume(&self, sweeper: &Self::Sweeper) -> Result<(), String>;
    /// Ends it after a failed step.
    fn kill(&self, sweeper: &Self::Sweeper);
    /// Keeps it, so the outer can wait for it if it lives that long.
    fn keep(&self, sweeper: Self::Sweeper);
    /// A line on the outer's stderr.
    fn log(&self, line: &str);
    /// The time on a monotonic clock, from any fixed origin: how long ago the host's stdin ended.
    fn now(&self) -> Duration;
    /// At the outer's end: waits a while for the kept sweeper and says how it ended
    /// ([`Swept::from_exit`]); [`Swept::Unclean`] when none was kept.
    fn wait(&self) -> Swept;
}

/// How the end sweeper ended, as far as the outer saw.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Swept {
    /// It exited 0: every kill and removal went.
    Clean,
    /// It exited [`RunExit::Escapee`]: a process of the pool user's outlived its sweep.
    Escapee,
    /// It failed otherwise (a [`RunExit::PoolStop`] exit among them, which no sweeper gives any
    /// more), was still running when the outer's wait ran out, or never started.
    Unclean,
}

impl Swept {
    /// From the sweeper's exit code, `None` when it hadn't exited.
    pub(crate) fn from_exit(code: Option<u32>) -> Swept {
        match code {
            Some(0) => Swept::Clean,
            Some(code) if code == RunExit::Escapee.code() => Swept::Escapee,
            Some(_) | None => Swept::Unclean,
        }
    }
}

/// Whether the sweeper's start still fits once `since` has passed since the host's stdin ended:
/// at least [`SWEEPER_BUDGET`] of [`HOST_BUDGET`] is left, so the host's kill of the outer can't
/// come between the sweeper's logon and its resume in the normal timing.
pub(crate) fn sweeper_fits(since: Duration) -> bool {
    HOST_BUDGET.saturating_sub(since) >= SWEEPER_BUDGET
}

/// Starts the sweeper: the job emptied, then logon, shut, resume, and nothing else (no job). A
/// job that doesn't empty starts no sweeper: a process of the agent's would be alive in the
/// sweeper's logon gap. Nor does a start too late after the host's stdin ended (`stdin_ended`,
/// on [`SweeperSteps::now`]'s clock; [`sweeper_fits`]). A failure is logged. Whether it started.
pub(crate) fn start_sweeper<S: SweeperSteps>(steps: &S, stdin_ended: Option<Duration>) -> bool {
    if let Err(what) = steps.empty_job() {
        return no_sweeper(steps, &what);
    }
    if let Some(ended) = stdin_ended {
        let since = steps.now().saturating_sub(ended);
        if !sweeper_fits(since) {
            let left = HOST_BUDGET.saturating_sub(since).as_millis();
            return no_sweeper(
                steps,
                &format!(
                    "skipped for time: {left} ms of the host's {} ms after the stdin end were \
                     left, under the {} ms the sweeper's start needs",
                    HOST_BUDGET.as_millis(),
                    SWEEPER_BUDGET.as_millis()
                ),
            );
        }
    }
    let sweeper = match steps.log_on() {
        Ok(sweeper) => sweeper,
        Err(what) => {
            steps.log(&format!(
                "clauseo-sandbox-run: the end sweeper didn't start: {what}"
            ));
            return false;
        }
    };
    if let Err(what) = steps.shut(&sweeper).and_then(|()| steps.resume(&sweeper)) {
        steps.kill(&sweeper);
        steps.log(&format!(
            "clauseo-sandbox-run: the end sweeper didn't start: {what}"
        ));
        return false;
    }
    steps.keep(sweeper);
    true
}

/// No sweeper this session: says why on stderr, as a warning. The session isn't clean, so its
/// unclean-session marker stays. False, for [`start_sweeper`].
fn no_sweeper<S: SweeperSteps>(steps: &S, what: &str) -> bool {
    steps.log(&format!(
        "clauseo-sandbox-run: warning: no end sweeper: {what}; the next launch's first act sweeps"
    ));
    false
}

/// Asks `active` for the job's process count until it is 0, every `poll` (through `pause`)
/// for up to `wait`: [`SweeperSteps::empty_job`]'s wait once the job is ended. An error names
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

/// The guard that starts the sweeper once a session.
pub(crate) struct EndSweep<S> {
    steps: S,
    /// Whether the sweeper started, once a path's start of it is over.
    started: OnceLock<bool>,
    /// When the host's stdin ended, on [`SweeperSteps::now`]'s clock.
    stdin_ended: OnceLock<Duration>,
    /// Whether a sweep of the session found a process of the pool user's it couldn't end.
    escaped: AtomicBool,
    /// The end signal of the path that began the end, set before that path ends the job.
    begun: Mutex<Option<EndSignal>>,
    begun_changed: Condvar,
}

impl<S: SweeperSteps> EndSweep<S> {
    pub(crate) fn new(steps: S) -> EndSweep<S> {
        EndSweep {
            steps,
            started: OnceLock::new(),
            stdin_ended: OnceLock::new(),
            escaped: AtomicBool::new(false),
            begun: Mutex::new(None),
            begun_changed: Condvar::new(),
        }
    }

    /// Empties the job and starts the sweeper unless a path did already; whether this call began
    /// the end. A call while another path's start is still going returns once that start is
    /// over, so no path goes on (the outer's exit among them) with the job still being ended or
    /// the sweeper half started.
    pub(crate) fn trigger(&self, why: EndSignal) -> bool {
        let first = self.begin(why);
        self.start_once();
        first
    }

    /// The git watch's end: begins the end (`why`), ends the job and waits, up to
    /// [`EMPTY_WAIT`], until no process is left in it ([`SweeperSteps::empty_job`]), whichever
    /// path began the end. Whether it is empty: only then may the watch move anything. The
    /// sweeper's logon, which can take hundreds of milliseconds, comes after, on a thread of its
    /// own ([`EndSweep::trigger`]'s start, once a session; this thread's when none starts), so it
    /// never holds up the moves; a path that ends the session meanwhile waits for it as ever.
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
            .name("end-sweeper".to_owned())
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

    /// Empties the job and starts the sweeper, once a session ([`start_sweeper`]); a call
    /// while another's is going waits for it.
    fn start_once(&self) {
        self.started
            .get_or_init(|| start_sweeper(&self.steps, self.stdin_ended.get().copied()));
    }

    /// The host's stdin ended now: from here the host gives the outer [`HOST_BUDGET`], and a
    /// sweeper starts only while [`sweeper_fits`], on whichever path. The first call counts.
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

    /// A sweep found an escapee: the inner reported one on `ctl`, or the end sweeper exited so.
    pub(crate) fn mark_escapee(&self) {
        self.escaped.store(true, Ordering::SeqCst);
    }

    /// Whether a sweep of the session found an escapee.
    pub(crate) fn escaped(&self) -> bool {
        self.escaped.load(Ordering::SeqCst)
    }

    /// At the outer's end: waits a while for the sweeper ([`SweeperSteps::wait`]), keeping an
    /// escapee it found.
    pub(crate) fn wait(&self) -> Swept {
        let swept = self.steps.wait();
        if swept == Swept::Escapee {
            self.mark_escapee();
        }
        swept
    }

    /// Whether the session ended clean, so the outer removes its unclean-session marker
    /// (`marker.rs`): its end began at the inner's exit (no failure, and no cancel whose agent
    /// outlived [`GRACE`]: an agent that finished after the host's stdin ended, its output
    /// drained before the grace ran out, ends clean), the inner reported the agent's exit
    /// (`reported`), the end sweeper started (the job emptied first) and exited 0 (`swept`), and
    /// no sweep found an escapee.
    pub(crate) fn clean(&self, swept: Swept, reported: bool) -> bool {
        self.began() == Some(EndSignal::InnerExited)
            && reported
            && swept == Swept::Clean
            && !self.escaped()
    }

    /// The outer's exit code for a run that would exit `code`: [`RunExit::Escapee`] when a sweep
    /// of the session found an escapee, whatever `code` is (the agent's own, even 0, which the
    /// outer then says on stderr); else `code`.
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
/// stdin, [`EndSignal::Stdin`]), the outer ends its job ([`SweeperSteps::empty_job`]), and the
/// inner and the agent with it, with [`SWEPT`], the end sweeper's own kill code, which would come
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
/// ([`EndSweep::stdin_ended`]) at once, even while the writer is still blocked on an agent that
/// isn't reading, so the deadline counts from the host's real end. The writer then hands over
/// what is left and closes the agent's stdin (`close`), so the agent reads the end of its input;
/// the agent has what is left of `grace` ([`GRACE`]), counted from the stdin end, to finish:
/// the inner's exit begins the end meanwhile ([`inner_exited`]), its output and code kept. When
/// no path began it by then, this one does ([`EndSignal::Stdin`]): the job ends, and the agent
/// with it, inside the host's 2 s; a writer still blocked then fails, its pipe's other end gone
/// with the job, and closes. An agent that closed its own stdin ends nothing: the writer stops,
/// and the host's stdin is still read, its end still kept and acted on. Past [`STDIN_PIECES`]
/// pieces the agent hasn't read, the read waits for the writer, so the end is seen late.
pub(crate) fn relay_stdin<S: SweeperSteps, W: Write + Send>(
    mut from: impl Read,
    to: W,
    end: &EndSweep<S>,
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
/// stream), then the job's end and the sweeper ([`EndSignal::InnerExited`], unless the stdin's
/// grace ran out first), then `drain` again for what a descendant left in the job held. The exit
/// code ([`exit_code`]).
pub(crate) fn inner_exited<S: SweeperSteps>(
    end: &EndSweep<S>,
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
