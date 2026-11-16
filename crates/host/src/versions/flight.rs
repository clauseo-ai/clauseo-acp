//! The session's one job on the matter's thread at a time ([`Flight`]), and how a call runs a
//! read there and waits for it ([`Backend::on_matter`]).
//!
//! A call first waits for the session's earlier job to end, then queues its own, all within one
//! [`MATTER_WAIT`] from its start. The job holds the session's slot until it has run or been
//! dropped, so a call that gave up still keeps the next one waiting until its job is off the
//! matter's thread; and a job whose caller gave up (its `gone` flag) does nothing when its turn
//! comes. So one session puts at most one read at a time in front of a checkpoint, an undo or a
//! restore, and a read nobody waits for costs nothing.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::time::Duration;

use super::texts::Refusal;
use super::{Backend, MATTER_WAIT, MatterRead, Opened, Unreachable, lock};

/// Whether the session has a job on the matter's thread.
#[derive(Debug, Default)]
pub(super) struct Flight {
    busy: Mutex<bool>,
    freed: Condvar,
    /// How long a call waits: [`MATTER_WAIT`], shorter in tests.
    wait: Mutex<Option<Duration>>,
    /// Reads that ran on the matter's thread (tests).
    #[cfg(test)]
    pub(super) ran: std::sync::atomic::AtomicUsize,
}

impl Flight {
    /// Calls wait `wait` instead of [`MATTER_WAIT`] (tests).
    #[cfg(test)]
    pub(super) fn set_wait(&self, wait: Duration) {
        *lock(&self.wait) = Some(wait);
    }
}

/// The session's slot, held by one job; dropping it frees the slot.
struct Ticket(Arc<Flight>);

impl Drop for Ticket {
    fn drop(&mut self) {
        *lock(&self.0.busy) = false;
        self.0.freed.notify_one();
    }
}

impl Backend {
    /// Runs `read` with this session's matter on its thread, once the session's earlier job is
    /// done, and waits for its answer, all within [`MATTER_WAIT`]. A matter not open yet is
    /// [`Refusal::NotOpen`]; a thread that cannot take the read or drops it, and no answer in
    /// time, are [`Refusal::Unavailable`].
    pub(super) fn on_matter<T: Send + 'static>(
        &self,
        read: impl FnOnce(Opened<'_>) -> Result<T, Refusal> + Send + 'static,
    ) -> Result<T, Refusal> {
        let clock = &self.config.clock;
        let wait = lock(&self.flight.wait).unwrap_or(MATTER_WAIT);
        let deadline = clock.now().mono.saturating_add(wait);
        let left = || deadline.saturating_sub(clock.now().mono);
        let ticket = self.enter(left)?;
        let gone = Arc::new(AtomicBool::new(false));
        let (answer, answered) = mpsc::sync_channel(1);
        let job_gone = Arc::clone(&gone);
        let job = MatterRead(Box::new(move |opened: Option<Opened<'_>>| {
            if job_gone.load(Ordering::SeqCst) {
                return;
            }
            #[cfg(test)]
            ticket.0.ran.fetch_add(1, Ordering::SeqCst);
            let result = opened.map_or(Err(Refusal::NotOpen), read);
            // The caller may have stopped waiting meanwhile; nobody is left to tell.
            let _gone = answer.send(result);
            drop(ticket);
        }));
        if self
            .config
            .source
            .on_matter(self.config.session.matter, job)
            == Err(Unreachable)
        {
            return Err(Refusal::Unavailable);
        }
        let answer = answered.recv_timeout(left());
        if answer.is_err() {
            gone.store(true, Ordering::SeqCst);
        }
        answer.unwrap_or(Err(Refusal::Unavailable))
    }

    /// Takes the session's slot, waiting while an earlier job holds it, at most `left()`.
    fn enter(&self, left: impl Fn() -> Duration) -> Result<Ticket, Refusal> {
        let mut busy = lock(&self.flight.busy);
        while *busy {
            let wait = left();
            if wait.is_zero() {
                return Err(Refusal::Unavailable);
            }
            busy = self
                .flight
                .freed
                .wait_timeout(busy, wait)
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        *busy = true;
        drop(busy);
        Ok(Ticket(Arc::clone(&self.flight)))
    }
}
