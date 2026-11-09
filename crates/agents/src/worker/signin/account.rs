//! The worker's share of an account read (`Agents::read_account`, R17): a job of its own that
//! takes no slot, so it never replaces or waits for the agent's sign-in job and none of its
//! events reach the sign-in state (its ticket is never the agent's job, so `emit_job` drops
//! anything it sends). Its cancel is kept with the hub's, so quitting stops it as it stops a
//! sign-in job.

use std::sync::{Arc, Weak};

use clauseo_process::Timer;
use clauseo_protocol::AgentId;
use futures::channel::oneshot;

use super::super::{Ticket, Worker};
use super::{SigninHub, SigninStart};
use crate::cancel::Cancel;
use crate::signin::SigninError;
use crate::signin::account::{self, AccountFuture, AccountRead, ReadOrder};
use crate::signin::job::{SigninJob, Task};
use crate::state::Installed;

/// The read's checks on `hub` (a known agent with a home and a ready install), then a ticket
/// that takes no slot.
pub(in crate::worker) fn reserve_read(
    hub: &SigninHub,
    agent: &AgentId,
    installed: Option<Installed>,
) -> Result<SigninStart, SigninError> {
    let vendor = hub.vendor(agent)?;
    let home = hub
        .agents
        .get(agent)
        .and_then(|known| known.home.clone())
        .ok_or_else(|| SigninError::NoHome(agent.clone()))?;
    if installed.is_none() {
        return Err(SigninError::NotReady(agent.clone()));
    }
    let mut inner = hub.lock();
    inner.next_job += 1;
    let cancel = Arc::new(Cancel::new());
    inner.reads.retain(|read| read.strong_count() > 0);
    inner.reads.push(Arc::downgrade(&cancel));
    let ticket = Ticket {
        job: inner.next_job,
        agent: agent.clone(),
        cancel,
    };
    Ok(SigninStart {
        ticket,
        task: Task::Status,
        vendor,
        installed,
        home,
        paste: None,
        kick: None,
    })
}

/// A read's order for its job, with refresh-on-check's `refresh`, and the let-go its caller's
/// future sets ([`answer_of`]).
pub(in crate::worker) fn read_order(hub: &SigninHub, refresh: bool) -> (ReadOrder, Arc<Cancel>) {
    let let_go = Arc::new(Cancel::new());
    let order = ReadOrder {
        refresh,
        let_go: Arc::clone(&let_go),
        forced: Arc::clone(&hub.forced_runs),
    };
    (order, let_go)
}

/// The caller's side of a read: the answer `reply` gets, `Failed` if none comes. Dropped before
/// the answer (R17's check gave a forced read up at its bound), it sets `let_go`: a forced
/// read's app-server then gets its stdin closed and ends on its own, never killed but at quit
/// (`codex::identity`), since Codex may have rotated the refresh token and not yet saved it.
pub(in crate::worker) fn answer_of(
    answer: oneshot::Receiver<AccountRead>,
    let_go: Arc<Cancel>,
) -> AccountFuture {
    let mut guard = LetGoOnDrop(Some(let_go));
    Box::pin(async move {
        let read = answer.await.unwrap_or(AccountRead::Failed);
        guard.disarm();
        read
    })
}

/// Lets its read go when dropped.
struct LetGoOnDrop(Option<Arc<Cancel>>);

impl LetGoOnDrop {
    /// Answered: nothing is left to let go.
    fn disarm(&mut self) {
        self.0 = None;
    }
}

impl Drop for LetGoOnDrop {
    fn drop(&mut self) {
        if let Some(let_go) = &self.0 {
            let_go.cancel();
        }
    }
}

/// Cancels every account read still running on `hub`: the handle is gone.
pub(super) fn stop_reads(hub: &SigninHub) {
    let reads: Vec<Weak<Cancel>> = std::mem::take(&mut hub.lock().reads);
    for cancel in reads.iter().filter_map(Weak::upgrade) {
        cancel.cancel();
    }
}

/// Starts the read on its own thread; its answer goes to `reply`. A thread that does not start
/// drops `reply`, which reads as [`AccountRead::Failed`].
pub(in crate::worker) fn start_account<T: Timer + Send + Sync + 'static>(
    worker: &Worker<T>,
    start: SigninStart,
    order: ReadOrder,
    reply: oneshot::Sender<AccountRead>,
) {
    let job = SigninJob {
        ticket: start.ticket,
        task: start.task,
        vendor: start.vendor,
        config: worker.config.clone(),
        installed: start.installed,
        home: start.home,
        browser: None,
        launcher: Arc::clone(&worker.signin_launcher),
        timer: Arc::clone(&worker.timer),
        shared: Arc::clone(&worker.shared),
        paste: None,
        status_limit: std::sync::Mutex::new(None),
    };
    let name = format!("clauseo-account-{}", job.ticket.agent);
    // A thread that did not start dropped `reply` with its closure: the caller reads `Failed`.
    drop(worker.shared.spawn_job(name, move || {
        let read = futures::executor::block_on(account::read(&job, &order));
        // The caller may have stopped waiting.
        let _gone = reply.send(read);
    }));
}
