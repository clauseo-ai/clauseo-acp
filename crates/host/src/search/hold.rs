//! [`SearchHold`]: a test seam that stops the search thread at one point until released.

/// Stops the search thread at one point (before each conversation of a rebuild, or before each
/// query) until [`SearchHold::release`]; [`SearchHold::reached`] waits until the thread got
/// there. Clones share the hold. A test seam: only `HostConfig::with_search_holds` (feature
/// `test-support`) takes one.
#[derive(Debug, Clone)]
pub struct SearchHold {
    /// Closed by `release`: the thread waits on it while it is open.
    release: async_channel::Sender<()>,
    released: async_channel::Receiver<()>,
    /// The thread sends one note each time it stops at the hold.
    reached_tx: async_channel::Sender<()>,
    reached_rx: async_channel::Receiver<()>,
}

impl SearchHold {
    /// A hold that stops the thread until released.
    #[must_use]
    pub fn new() -> Self {
        let (release, released) = async_channel::unbounded();
        let (reached_tx, reached_rx) = async_channel::unbounded();
        Self {
            release,
            released,
            reached_tx,
            reached_rx,
        }
    }

    /// Lets the thread go on, now and at every later stop.
    pub fn release(&self) {
        self.release.close();
    }

    /// Waits until the thread stopped at the hold once more since the last call returned.
    pub async fn reached(&self) {
        let _note = self.reached_rx.recv().await;
    }

    /// Whether the thread may go on.
    pub(crate) fn is_released(&self) -> bool {
        self.release.is_closed()
    }

    /// Notes that the thread stopped here.
    pub(crate) fn note_reached(&self) {
        let _full = self.reached_tx.try_send(());
    }

    /// Waits for the release (on the search thread).
    pub(crate) async fn wait(&self) {
        while self.released.recv().await.is_ok() {}
    }
}

impl Default for SearchHold {
    fn default() -> Self {
        Self::new()
    }
}
