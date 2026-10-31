//! The snapshot thread, `clauseo-snapshot`: compresses thread snapshots off the actor.
//!
//! The actor encodes a snapshot to `MessagePack` from a borrow of its state
//! (`reducer::Snapshot::of(state).to_bytes()`), since only it holds the state and a clone would
//! cost a second copy. Compression is most of the cost (16 to 20 ms of 20 ms for a 10 MB state,
//! measured on an Apple M6), so the bytes go to this one long-lived thread, which runs
//! `SnapshotIn::compress` and posts the result back; the actor sends it as a commit with no
//! events. One thread keeps snapshots in order, one at a time.

use std::sync::mpsc;
use std::thread::{self, JoinHandle};

use clauseo_protocol::{Seq, StreamId};
use clauseo_reducer::REDUCER_SCHEMA_VERSION;
use clauseo_store::SnapshotIn;

use crate::error::Error;

/// The name of the snapshot thread.
pub(crate) const THREAD_NAME: &str = "clauseo-snapshot";

/// One snapshot to compress: the stream, the seq it was taken at, and its encoded bytes.
struct Job {
    stream: StreamId,
    seq: Seq,
    bytes: Vec<u8>,
}

/// The handle of the snapshot thread.
pub(crate) struct SnapshotWorker {
    /// Jobs in submit order; dropping it lets the thread finish the queue and end.
    jobs: mpsc::Sender<Job>,
    /// The thread, joined by [`SnapshotWorker::stop`].
    thread: JoinHandle<()>,
}

impl SnapshotWorker {
    /// Starts the thread. `post` gets each compressed snapshot, in submit order, on the
    /// snapshot thread; it hands it to the actor's input channel.
    pub(crate) fn start(post: Box<dyn Fn(SnapshotIn) + Send>) -> Result<Self, Error> {
        let (jobs, queue) = mpsc::channel::<Job>();
        let thread = thread::Builder::new()
            .name(THREAD_NAME.to_owned())
            .spawn(move || {
                for job in queue {
                    post(SnapshotIn::compress(
                        job.stream,
                        job.seq,
                        REDUCER_SCHEMA_VERSION,
                        &job.bytes,
                    ));
                }
            })
            .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))?;
        Ok(Self { jobs, thread })
    }

    /// Queues the snapshot of `stream` at `seq`, already encoded on the actor, for compression.
    /// Fails with [`Error::Closed`] when the thread has ended.
    pub(crate) fn submit(&self, stream: StreamId, seq: Seq, bytes: Vec<u8>) -> Result<(), Error> {
        self.jobs
            .send(Job { stream, seq, bytes })
            .map_err(|_ended| Error::Closed)
    }

    /// Finishes the snapshots already queued, then joins the thread. Shutdown calls it before it
    /// closes the store, so a snapshot in flight is posted first.
    pub(crate) fn stop(self) -> Result<(), Error> {
        let Self { jobs, thread } = self;
        drop(jobs);
        thread
            .join()
            .map_err(|_panic| Error::Internal(format!("{THREAD_NAME} panicked")))
    }
}

#[cfg(test)]
mod tests;
