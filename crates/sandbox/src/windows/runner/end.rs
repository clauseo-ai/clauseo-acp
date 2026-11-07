//! The detached end sweeper's start (M1), apart from the outer's calls so the tests hold it on
//! every OS.
//!
//! The outer starts it as its first reaction to any end signal: the host's stdin ending (the
//! host's cancel ladder closes it, then gives the outer 2 s before it ends the outer's job), the
//! inner runner exiting, or any failure once the launch is read. [`EndSweep::trigger`] starts it
//! once a session, whichever path comes first. It is started (`CreateProcessWithLogonW`,
//! suspended), its process and thread are shut as the inner's are, and it is resumed: it is never
//! put in this launch's kill-on-close job, so the host ending the outer leaves it running. It
//! needs nothing from the outer once running (`--sweep`, its own timeout).

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// The calls that start the sweeper; Windows' are `outer/end.rs`'s.
pub(crate) trait SweeperSteps {
    /// The suspended sweeper.
    type Sweeper;
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

/// Starts the sweeper: logon, shut, resume, and nothing else (no job). A failure is logged.
pub(crate) fn start_sweeper<S: SweeperSteps>(steps: &S) {
    let sweeper = match steps.log_on() {
        Ok(sweeper) => sweeper,
        Err(what) => {
            steps.log(&format!(
                "clauseo-sandbox-run: the end sweeper didn't start: {what}"
            ));
            return;
        }
    };
    if let Err(what) = steps.shut(&sweeper).and_then(|()| steps.resume(&sweeper)) {
        steps.kill(&sweeper);
        steps.log(&format!(
            "clauseo-sandbox-run: the end sweeper didn't start: {what}"
        ));
        return;
    }
    steps.keep(sweeper);
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
    started: AtomicBool,
}

impl<S: SweeperSteps> EndSweep<S> {
    pub(crate) fn new(steps: S) -> EndSweep<S> {
        EndSweep {
            steps,
            started: AtomicBool::new(false),
        }
    }

    /// Starts the sweeper unless a path did already; whether this call started it.
    pub(crate) fn trigger(&self, _why: EndSignal) -> bool {
        if self.started.swap(true, Ordering::SeqCst) {
            return false;
        }
        start_sweeper(&self.steps);
        true
    }

    /// The steps, for the wait at the outer's end.
    pub(crate) fn steps(&self) -> &S {
        &self.steps
    }
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

/// The host's stdin relayed to the agent's: when the host's side ends, the sweeper is started
/// first, then the agent's stdin is closed (`close`).
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
