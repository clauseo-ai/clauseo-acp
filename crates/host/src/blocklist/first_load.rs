//! The block list's first load, as launches wait for it: the updater tells [`FirstLoad`] once
//! its start-up load has offered the bundled snapshot (`Updater::load_at_start`), and a launch
//! whose session gets a proxy waits for that, at most [`FIRST_LOAD_WAIT`] on the host clock,
//! before it opens the proxy (`launch_checks/egress.rs`). Only with a bundled snapshot: without
//! one there is nothing quick to wait for. Any number of launches may wait on the one signal; an
//! updater that never ran (its thread refused) drops the teller, which ends every wait at once.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use clauseo_blocklist::Current;
use futures::channel::oneshot;
use futures::future::{Either, FutureExt as _, Shared, select};

use crate::clock::Clock;

/// The most a launch waits for the first load: about the bundled snapshot's read and verify on
/// a slow disk. Past it the launch goes on with the list in use (ADR 0053 accepts the window),
/// with one warning. 4 s, so no other host timer has its delay (the tests find it by delay).
pub(crate) const FIRST_LOAD_WAIT: Duration = Duration::from_secs(4);

/// The first load's signal: launches wait on `done`; the updater takes `tell` once.
#[derive(Clone)]
pub(crate) struct FirstLoad {
    done: Shared<oneshot::Receiver<()>>,
    tell: Arc<Mutex<Option<oneshot::Sender<()>>>>,
}

impl FirstLoad {
    pub(crate) fn new() -> Self {
        let (tell, done) = oneshot::channel();
        Self {
            done: done.shared(),
            tell: Arc::new(Mutex::new(Some(tell))),
        }
    }

    /// The updater's half, the first time it is asked for.
    pub(crate) fn teller(&self) -> Option<oneshot::Sender<()>> {
        self.tell
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
    }

    /// Waits for the first load, at most [`FIRST_LOAD_WAIT`] on `clock`. When the wait timed
    /// out or `current` holds no list after it, one warning (no path, no host).
    pub(crate) async fn wait(self, clock: Arc<dyn Clock>, current: Arc<Current>) {
        let timed_out = matches!(
            select(self.done, clock.after(FIRST_LOAD_WAIT)).await,
            Either::Right(_)
        );
        if timed_out || current.get().is_unloaded() {
            tracing::warn!(
                timed_out,
                "egress: the block list was not loaded yet; the session starts with the list in use"
            );
        }
    }
}
