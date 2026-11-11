//! What the quit needs of a stopped sign-in job: a mark its thread sets as it ends
//! ([`JobEnd`]), and the job's stop, to wait on and to kill at the bound ([`Stopped`]).

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use super::{Shared, Ticket};
use crate::cancel::Cancel;
use crate::signin::{SigninCode, SigninState};

/// Set once when a job's thread ends; [`JobEnd::wait`] waits for it.
#[derive(Debug, Default)]
pub(crate) struct JobEnd {
    done: Mutex<bool>,
    ended: Condvar,
}

impl JobEnd {
    /// Marks the thread ended and wakes every wait.
    pub(crate) fn mark(&self) {
        *self.done.lock().unwrap_or_else(PoisonError::into_inner) = true;
        self.ended.notify_all();
    }

    /// Waits at most `bound` for the end; whether it came.
    pub(crate) fn wait(&self, bound: Duration) -> bool {
        let done = self.done.lock().unwrap_or_else(PoisonError::into_inner);
        let (done, _timed_out) = self
            .ended
            .wait_timeout_while(done, bound, |done| !*done)
            .unwrap_or_else(PoisonError::into_inner);
        *done
    }
}

/// One stopped job, for the quit to wait on and to kill at its bound.
#[derive(Debug)]
pub(crate) struct Stopped {
    cancel: Arc<Cancel>,
    ended: Arc<JobEnd>,
}

impl Stopped {
    /// A job's stop: its cancel and its end mark.
    pub(crate) fn new(cancel: Arc<Cancel>, ended: Arc<JobEnd>) -> Stopped {
        Stopped { cancel, ended }
    }

    /// Waits at most `bound` for the job's thread to end; if it has not, kills its runs (the
    /// stop at once). Whether it had ended.
    pub(crate) fn wait_or_kill(&self, bound: Duration) -> bool {
        let ended = self.ended.wait(bound);
        if !ended {
            self.cancel.cancel();
        }
        ended
    }
}

/// Marks a sign-in job's thread ended as it ends, a panic included; after a panic (a dev or test
/// build; a release build aborts) the job's slot goes too, with `Failed`, so the agent's next
/// sign-in and another agent's waiting press (desktop's one sign-in at a time) are not held.
pub(super) struct SigninEnd {
    pub(super) shared: Arc<Shared>,
    pub(super) ticket: Ticket,
    pub(super) ended: Arc<JobEnd>,
    pub(super) clock: Arc<dyn crate::config::Clock>,
}

impl Drop for SigninEnd {
    fn drop(&mut self) {
        if std::thread::panicking() {
            tracing::warn!(agent = %self.ticket.agent, "agent sign-in: the job's thread panicked");
            let state = SigninState::Failed {
                code: SigninCode::Failed,
                at: self.clock.now(),
            };
            self.shared.signin.emit_job(&self.ticket, state, true);
        }
        self.ended.mark();
    }
}
