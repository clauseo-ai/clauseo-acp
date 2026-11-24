//! A running turn's tool-call places, shared with its matter's thread: the actor notes each
//! call as it arrives, and a checkpoint job of another turn of the matter reads them when it
//! runs, not when it was sent, so a write that starts in between is seen
//! (`matter_worker/beside.rs`).

use std::sync::{Arc, Mutex, PoisonError};

use super::{CallFields, ShellStart, TurnPaths};

/// One turn's [`TurnPaths`], behind a lock both threads take briefly.
#[derive(Debug, Clone, Default)]
pub(crate) struct SharedPaths(Arc<Mutex<TurnPaths>>);

impl SharedPaths {
    /// `paths`, shared.
    pub(crate) fn new(paths: TurnPaths) -> Self {
        Self(Arc::new(Mutex::new(paths)))
    }

    /// Runs `then` on the places; a panic elsewhere while the lock was held leaves them as
    /// they were, still read.
    fn with<T>(&self, then: impl FnOnce(&mut TurnPaths) -> T) -> T {
        let mut paths = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        then(&mut paths)
    }

    /// [`TurnPaths::note`].
    pub(crate) fn note(&self, key: &str, fields: &CallFields<'_>) {
        self.with(|paths| paths.note(key, fields));
    }

    /// [`TurnPaths::set_start`].
    pub(crate) fn set_start(&self, start: ShellStart) {
        self.with(|paths| paths.set_start(start));
    }

    /// [`TurnPaths::tool_name`], owned.
    pub(crate) fn tool_name(&self, key: &str) -> Option<String> {
        self.with(|paths| paths.tool_name(key).map(str::to_owned))
    }

    /// The places as they are now.
    pub(crate) fn snapshot(&self) -> TurnPaths {
        self.with(|paths| paths.clone())
    }
}
