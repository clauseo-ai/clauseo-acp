//! The search thread, `clauseo-search` (decision 0043): it owns the `SearchIndex` in the data
//! folder and feeds it from committed events; the actor never touches the index.
//!
//! The actor hands each returned commit's envelopes over a bounded channel with `try_send` and
//! never waits ([`Search::offer`]). When the channel is full it sets the lagged flag instead,
//! and the thread catches up from its per-stream watermarks by reading the store. Searches,
//! forgets and re-imports go over an unbounded control channel the thread reads first, so a
//! search is answered between two conversations of a rebuild.
//!
//! A newer search from the same window supersedes an older one ([`Generations`]): it interrupts
//! the older query if that one is running, an older one still queued is answered `Cancelled`,
//! and an older one that finishes late is answered `Cancelled` instead of its page.

mod answer;
mod batch;
mod files;
mod fold;
mod hold;
mod worker;

#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};

use clauseo_protocol::{Envelope, MatterId, SearchAnswer, SearchRequest};
use clauseo_search::Interrupter;
use clauseo_store::Store;

use crate::clock::Clock;
use crate::error::Error;

pub use hold::SearchHold;

/// The name of the search thread.
pub(crate) const THREAD_NAME: &str = "clauseo-search";

/// Commits the channel from the actor holds before the thread lags and catches up from the
/// store.
pub(crate) const COMMIT_CAPACITY: usize = 1_024;

/// The window key a search sent through `HostHandle::query` is superseded under.
pub(crate) const QUERY_WINDOW: &str = "query";

/// The test seams: the commit channel's capacity and the holds that stop the thread.
#[derive(Debug, Clone)]
pub(crate) struct SearchSeams {
    pub(crate) capacity: usize,
    /// Stops the rebuild before each conversation until released.
    pub(crate) rebuild_hold: Option<SearchHold>,
    /// Stops the thread before each query runs until released.
    pub(crate) query_hold: Option<SearchHold>,
}

impl Default for SearchSeams {
    fn default() -> Self {
        Self {
            capacity: COMMIT_CAPACITY,
            rebuild_hold: None,
            query_hold: None,
        }
    }
}

/// Where a search's answer goes: the caller's reply, mapped to its type.
pub(crate) type SearchReply = Box<dyn FnOnce(Result<SearchAnswer, Error>) + Send>;

/// One job on the control channel.
pub(crate) enum Control {
    /// A search from `window`.
    Query {
        window: String,
        request: SearchRequest,
        reply: SearchReply,
    },
    /// The matter was taken off the list: its rows go.
    Forget(MatterId),
    /// The matter came back: its streams are indexed again from the log.
    Reindex(MatterId),
}

/// What the actor and the search thread share.
#[derive(Default)]
pub(crate) struct Shared {
    /// Set by the actor when the commit channel was full.
    pub(crate) lagged: AtomicBool,
    pub(crate) generations: Mutex<Generations>,
}

impl Shared {
    pub(crate) fn generations(&self) -> MutexGuard<'_, Generations> {
        self.generations
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Each window's newest search, and the query running now.
#[derive(Default)]
pub(crate) struct Generations {
    latest: BTreeMap<String, u64>,
    running: Option<(String, u64)>,
    interrupter: Option<Interrupter>,
}

impl Generations {
    /// A new search from `window`: `false` when a newer one came first (answer it
    /// `Cancelled`). Otherwise it becomes the window's newest, and an older one running now is
    /// interrupted.
    pub(crate) fn submit(&mut self, window: &str, generation: u64) -> bool {
        let latest = self.latest.entry(window.to_owned()).or_insert(0);
        if generation < *latest {
            return false;
        }
        *latest = generation;
        if let (Some((running, older)), Some(interrupter)) = (&self.running, &self.interrupter)
            && running == window
            && *older < generation
        {
            interrupter.interrupt();
        }
        true
    }

    /// Whether a newer search from `window` came after `generation`.
    pub(crate) fn superseded(&self, window: &str, generation: u64) -> bool {
        self.latest
            .get(window)
            .is_some_and(|&latest| latest > generation)
    }

    /// The thread starts running `generation` of `window`; `false` (and nothing runs) when it
    /// is already superseded.
    pub(crate) fn start(&mut self, window: &str, generation: u64) -> bool {
        if self.superseded(window, generation) {
            return false;
        }
        self.running = Some((window.to_owned(), generation));
        true
    }

    /// The running query returned.
    pub(crate) fn finish(&mut self) {
        self.running = None;
    }

    pub(crate) fn set_interrupter(&mut self, interrupter: Interrupter) {
        self.interrupter = Some(interrupter);
    }
}

/// The actor's handle of the search thread.
pub(crate) struct Search {
    commits: async_channel::Sender<Vec<Envelope>>,
    control: async_channel::Sender<Control>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Search {
    /// Starts the thread on the index in `data_dir`. It opens (or rebuilds) the index itself,
    /// so the host serves at once.
    pub(crate) fn start(
        data_dir: PathBuf,
        store: Store,
        clock: Arc<dyn Clock>,
        seams: SearchSeams,
    ) -> Result<Self, Error> {
        let (commits, commit_rx) = async_channel::bounded(seams.capacity.max(1));
        let (control, control_rx) = async_channel::unbounded();
        let shared = Arc::new(Shared::default());
        let parts = worker::Parts {
            data_dir,
            store,
            clock,
            shared: Arc::clone(&shared),
            commits: commit_rx,
            control: control_rx,
            seams,
        };
        let thread = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || worker::run(parts))
            .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))?;
        Ok(Self {
            commits,
            control,
            shared,
            thread: Some(thread),
        })
    }

    /// Hands a durable commit's envelopes to the thread without waiting: when the channel is
    /// full the thread is told it lagged and reads them from the store.
    pub(crate) fn offer(&self, envelopes: &[Envelope]) {
        if envelopes.is_empty() {
            return;
        }
        if let Err(async_channel::TrySendError::Full(_)) = self.commits.try_send(envelopes.to_vec())
        {
            self.shared.lagged.store(true, Ordering::SeqCst);
        }
    }

    /// Queues a search from `window`; a newer one from the same window came first, it is
    /// answered `Cancelled` at once.
    pub(crate) fn query(&self, window: &str, request: SearchRequest, reply: SearchReply) {
        if !self
            .shared
            .generations()
            .submit(window, request.generation.get())
        {
            reply(Ok(SearchAnswer::Cancelled));
            return;
        }
        let job = Control::Query {
            window: window.to_owned(),
            request,
            reply,
        };
        if let Err(error) = self.control.try_send(job)
            && let Control::Query { reply, .. } = error.into_inner()
        {
            reply(Err(Error::Internal(format!("{THREAD_NAME} has ended"))));
        }
    }

    /// The matter was forgotten: its rows go.
    pub(crate) fn forget(&self, matter: MatterId) {
        self.send(Control::Forget(matter));
    }

    /// The matter came back: index its streams again.
    pub(crate) fn reindex(&self, matter: MatterId) {
        self.send(Control::Reindex(matter));
    }

    fn send(&self, job: Control) {
        if self.control.try_send(job).is_err() {
            tracing::warn!("{THREAD_NAME} has ended; the index misses a change");
        }
    }

    /// Ends the thread after the job it is on and joins it. Shutdown calls it before it closes
    /// the store; dropping the handle (an abort) does the same.
    pub(crate) fn stop(self) {
        drop(self);
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        // Without `stop` (an abort) too: the thread ends at its next look at the channels, and
        // is joined so its connection and its clone of the store are closed when the host ends.
        self.commits.close();
        self.control.close();
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            tracing::warn!("{THREAD_NAME} panicked");
        }
    }
}
