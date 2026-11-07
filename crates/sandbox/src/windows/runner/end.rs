//! The detached end sweeper's start (M1), apart from the outer's calls so the tests hold it on
//! every OS.
//!
//! The outer starts it as its first reaction to any end signal: the host's stdin ending (the
//! host's cancel ladder closes it, then gives the outer 2 s before it ends the outer's job), the
//! inner runner exiting, or any failure once the launch is read. [`EndSweep::trigger`] starts it
//! once a session, whichever path comes first; a path that comes while the start is still going
//! waits for it. First the launch's kill-on-close job is ended and the outer waits, up to
//! [`EMPTY_WAIT`], until no process is left in it: the sweeper's logon opens a gap in which its
//! process, thread and token carry the logon's default DACL, and nothing of the agent's may be
//! alive then to open them. A job still holding a process by then starts no sweeper; the next
//! launch's first act is the backstop. Then the sweeper is started (`CreateProcessWithLogonW`,
//! suspended), its process and thread are shut as the inner's are, and it is resumed: it is never
//! put in this launch's kill-on-close job, so the host ending the outer leaves it running. It
//! needs nothing from the outer once running (`--sweep`, its own timeout). The guard keeps the
//! signal that started it, so a cancel exits as it would have without the sweeper
//! ([`exit_code`]).

use std::io::{self, Read, Write};
use std::sync::OnceLock;
use std::time::Duration;

use super::agent_code;
use super::sweep::SWEPT;

/// The exit code the host's own job end gives the outer, `TERMINATED` in
/// `crates/process/src/os/windows.rs:54` (private there): what a cancel whose agent didn't exit
/// within the host's 2 s gives without the end sweeper.
pub(crate) const HOST_ENDED: u32 = 0xC1A0;
/// How long the outer waits for its ended job to empty before it starts the sweeper: inside the
/// 2 s the host gives it after the stdin end, with the logon's start left room.
pub(crate) const EMPTY_WAIT: Duration = Duration::from_millis(1500);
/// How often it looks meanwhile.
pub(crate) const EMPTY_POLL: Duration = Duration::from_millis(25);

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
}

/// Starts the sweeper: the job emptied, then logon, shut, resume, and nothing else (no job). A
/// job that doesn't empty starts no sweeper: a process of the agent's would be alive in the
/// sweeper's logon gap. A failure is logged. Whether it started.
pub(crate) fn start_sweeper<S: SweeperSteps>(steps: &S) -> bool {
    if let Err(what) = steps.empty_job() {
        steps.log(&format!(
            "clauseo-sandbox-run: no end sweeper: {what}; the next launch's first act sweeps"
        ));
        return false;
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
    /// The end signal that started it, and whether the sweeper started.
    began: OnceLock<(EndSignal, bool)>,
}

impl<S: SweeperSteps> EndSweep<S> {
    pub(crate) fn new(steps: S) -> EndSweep<S> {
        EndSweep {
            steps,
            began: OnceLock::new(),
        }
    }

    /// Empties the job and starts the sweeper unless a path did already; whether this call did.
    /// A call while another path's start is still going returns once that start is over, so no
    /// path goes on (the outer's exit among them) with the job still being ended or the sweeper
    /// half started.
    pub(crate) fn trigger(&self, why: EndSignal) -> bool {
        let mut first = false;
        self.began.get_or_init(|| {
            first = true;
            (why, start_sweeper(&self.steps))
        });
        first
    }

    /// The end signal that started the sweeper, once one did.
    pub(crate) fn began(&self) -> Option<EndSignal> {
        self.began.get().map(|(why, _)| *why)
    }

    /// The steps, for the wait at the outer's end.
    pub(crate) fn steps(&self) -> &S {
        &self.steps
    }
}

/// The outer's exit code once the inner exited with `inner`, the agent having reported `reported`
/// on `ctl` if it exited first: the agent's code, never one in the runner's own range
/// ([`agent_code`]).
///
/// On a cancel (the session's end began at the host's stdin, [`EndSignal::Stdin`]), the outer
/// ends its job first ([`SweeperSteps::empty_job`]), and the inner and the agent with it, with
/// [`SWEPT`], the end sweeper's own kill code, which would come back as `AgentReserved`. The run
/// gives what it would have given without the job's end: the agent's own code when it exited
/// first and the inner reported it, or else [`HOST_ENDED`], the code of the host's job end that
/// follows a cancel whose agent didn't exit.
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

/// The host's stdin relayed to the agent's: when the host's side ends, the job is emptied and the
/// sweeper started first ([`EndSweep::trigger`]), then the agent's stdin is closed (`close`).
pub(crate) fn relay_stdin<S: SweeperSteps, W: Write>(
    from: impl Read,
    to: W,
    end: &EndSweep<S>,
    close: impl FnOnce(W),
) {
    let mut to = to;
    let ended = copy(from, &mut to);
    if ended == CopyEnd::Source {
        end.trigger(EndSignal::Stdin);
    }
    close(to);
}

#[cfg(test)]
mod tests;
