//! The matter list and the thread lists as app-wide events: [`MattersChanged`] and
//! [`ThreadsChanged`], which `subscribe_app` carries so the rail stays current without a
//! subscription per thread, and [`LastStop`], how a thread's last turn ended.

use serde::{Deserialize, Serialize};

use crate::{MatterId, MatterSummary, StopReason, ThreadId, ThreadSummary, Timestamp};

/// How and when a thread's last turn ended: its newest `TurnEnded`. A turn stopped at the limit
/// or failed ends with one too (`at_limit`, `failed`), so this covers every ending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LastStop {
    /// Why it stopped.
    pub reason: StopReason,
    /// The `TurnEnded` envelope's `ts`.
    pub at: Timestamp,
}

/// Matters on the list changed, came or went. JSON: `{"matters_changed": {...}}`.
///
/// With `all` false, `matters` holds only the rows that changed (each whole, replacing the
/// window's row of that matter, or added when new) and `removed` the matters that left the list
/// (`ForgetMatter`). With `all` true, `matters` is the whole list in `ListMatters`' order and
/// `removed` is empty: the window replaces its list and drops every row not in it. The first
/// frames of every app subscription, and the catch-up after a subscriber lagged, are the
/// `all: true` forms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct MattersChanged {
    /// The changed rows, or with `all` every row.
    pub matters: Vec<MatterSummary>,
    /// The matters that left the list; empty when none did, and always with `all`.
    pub removed: Vec<MatterId>,
    /// Whether `matters` is the whole list.
    pub all: bool,
}

/// Threads of one matter changed or came. JSON: `{"threads_changed": {...}}`.
///
/// Sent to every app subscriber, for every thread: the subscription knows no window, so a
/// window drops the rows of threads it has open (their own stream tells it more). With `all`
/// false, `threads` holds only the rows that changed or are new; with `all` true it is the
/// matter's whole thread list, as `ListThreads` answers, and the window drops every thread of
/// the matter not in it. A thread never leaves its matter's list (an archived thread stays, with
/// the `archived` status), so `removed` is always empty today; it is there so a later removal
/// needs no new shape. A matter that leaves the list takes its threads with it
/// (`MattersChanged::removed`), with no `ThreadsChanged`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ThreadsChanged {
    /// The matter the threads are in.
    pub matter: MatterId,
    /// The changed rows, or with `all` every row of the matter.
    pub threads: Vec<ThreadSummary>,
    /// Threads that left the matter's list; always empty today.
    pub removed: Vec<ThreadId>,
    /// Whether `threads` is the matter's whole list.
    pub all: bool,
}
