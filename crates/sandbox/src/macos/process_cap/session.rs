//! [`ProcessCapSession`]: the process cap's state for one lawyer session, which outlives each
//! launch (R17 makes one launcher per launch): how many times the cap tripped, which notices the
//! lawyer has had, and where they go.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// What the process cap tells the lawyer. Each one goes at most once per session. Append-only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ProcessCapNotice {
    /// A command's processes passed the cap and were stopped; the agent goes on.
    TooManyPrograms,
    /// The agent was ended too: it was the one forking, or the session tripped the cap a third
    /// time.
    AssistantStopped,
    /// The watchdog could no longer count the agent's processes (a listing or a read of the
    /// agent failed), so it ended the agent rather than leave it uncounted.
    CapFailed,
}

impl ProcessCapNotice {
    /// Its English text. [`ProcessCapNotice::TooManyPrograms`] is Windows' notice
    /// (`windows/runner/job_port.rs`), word for word.
    pub fn text(self) -> &'static str {
        match self {
            ProcessCapNotice::TooManyPrograms => {
                "A task tried to start too many programs at once, so Clauseo stopped it. Ask the \
                 assistant to try a smaller step."
            }
            ProcessCapNotice::AssistantStopped => {
                "A task kept starting too many programs at once, so Clauseo stopped the \
                 assistant. Start it again and ask for a smaller step."
            }
            ProcessCapNotice::CapFailed => {
                "Clauseo could no longer count the programs the assistant started, so it stopped \
                 the assistant. Start it again to go on."
            }
        }
    }

    /// Its bit in [`Inner::told`].
    fn bit(self) -> u8 {
        match self {
            ProcessCapNotice::TooManyPrograms => 1,
            ProcessCapNotice::AssistantStopped => 2,
            ProcessCapNotice::CapFailed => 4,
        }
    }
}

/// One lawyer session's process cap. The host makes one per session and gives a clone to each
/// launcher of that session (`SeatbeltLauncher::process_cap`), so the third trip ends the agent
/// and each notice comes once, however many launches the session makes. Cloning shares it.
#[derive(Clone)]
pub struct ProcessCapSession(Arc<Inner>);

/// The shared state.
struct Inner {
    /// The trips so far.
    trips: AtomicU32,
    /// The notices given, one bit each.
    told: AtomicU8,
    /// Where they go.
    notify: Box<dyn Fn(ProcessCapNotice) + Send + Sync>,
}

impl ProcessCapSession {
    /// A session whose notices go to `notify`, each at most once. `notify` must not block: the
    /// watchdog's thread calls it.
    pub fn new(notify: impl Fn(ProcessCapNotice) + Send + Sync + 'static) -> ProcessCapSession {
        ProcessCapSession(Arc::new(Inner {
            trips: AtomicU32::new(0),
            told: AtomicU8::new(0),
            notify: Box::new(notify),
        }))
    }

    /// How many times the cap tripped in this session.
    pub fn trips(&self) -> u32 {
        self.0.trips.load(Ordering::Relaxed)
    }

    /// Counts a trip; returns its number in the session, from 1.
    pub(crate) fn trip(&self) -> u32 {
        self.0
            .trips
            .fetch_add(1, Ordering::Relaxed)
            .saturating_add(1)
    }

    /// Gives `notice` unless the session had it already.
    pub(crate) fn tell(&self, notice: ProcessCapNotice) {
        let bit = notice.bit();
        if self.0.told.fetch_or(bit, Ordering::Relaxed) & bit == 0 {
            (self.0.notify)(notice);
        }
    }
}

impl std::fmt::Debug for ProcessCapSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessCapSession")
            .field("trips", &self.trips())
            .finish_non_exhaustive()
    }
}
