//! The requests forwarded to the relay that have no reply line yet. The stdin side adds one before
//! it writes the request; the relay side removes one when a reply with its id arrives; when the
//! link ends, every one left is answered with the not-reachable error. Each request is answered
//! once: whichever side removes it answers it, and a removal that finds nothing answers nothing.
//! The final drain closes the set, so a request added after it is refused at once and answered by
//! the stdin side: nothing is left to answer it later.

use std::collections::BTreeMap;
use std::iter;
use std::sync::{Mutex, MutexGuard, PoisonError};

use serde_json::Value;

/// The most requests in flight at once. A request past it is answered at once, not forwarded.
pub(crate) const MAX_PENDING: usize = 64;

/// The set of pending request ids. Ids are kept as their JSON text, so `1` and `"1"` differ; a
/// repeated id counts once per request.
#[derive(Default)]
pub(crate) struct Pending {
    inner: Mutex<Inner>,
}

/// What [`Pending::add`] did with a request.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Added {
    /// Recorded: forward it; whichever side releases it answers it.
    Pending,
    /// Not recorded: [`MAX_PENDING`] requests are already pending.
    Full,
    /// Not recorded: the link has ended and its pending requests have been answered.
    Closed,
}

#[derive(Default)]
struct Inner {
    /// Each id's JSON text, with its id value and how many requests carry it.
    by_text: BTreeMap<String, (Value, usize)>,
    /// The requests pending, all ids together.
    total: usize,
    /// Set by [`Pending::close`]: nothing is recorded any more.
    closed: bool,
}

impl Pending {
    /// Records a request with `id` before it is written; nothing is recorded when the set is full
    /// or closed.
    pub(crate) fn add(&self, id: &Value) -> Added {
        let mut inner = self.lock();
        if inner.closed {
            return Added::Closed;
        }
        if inner.total >= MAX_PENDING {
            return Added::Full;
        }
        inner
            .by_text
            .entry(id.to_string())
            .or_insert_with(|| (id.clone(), 0))
            .1 += 1;
        inner.total += 1;
        Added::Pending
    }

    /// Removes one pending request with `id`. `false` when none was pending: a reply to a request
    /// no longer pending, or a request already answered.
    pub(crate) fn release(&self, id: &Value) -> bool {
        let mut inner = self.lock();
        let text = id.to_string();
        let Some(entry) = inner.by_text.get_mut(&text) else {
            return false;
        };
        entry.1 -= 1;
        if entry.1 == 0 {
            inner.by_text.remove(&text);
        }
        inner.total -= 1;
        true
    }

    /// Removes every pending request, returns their ids once per request, and closes the set: a
    /// later [`Pending::add`] is [`Added::Closed`]. The link's end calls it once.
    pub(crate) fn close(&self) -> Vec<Value> {
        let mut inner = self.lock();
        inner.closed = true;
        inner.total = 0;
        std::mem::take(&mut inner.by_text)
            .into_values()
            .flat_map(|(id, count)| iter::repeat_n(id, count))
            .collect()
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // A panicked holder left the map whole; carry on.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests;
