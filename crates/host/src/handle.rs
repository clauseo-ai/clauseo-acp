//! The host's public surface: [`Host::start`], the [`HostHandle`] every caller holds, the
//! [`HostJoin`] that waits for the host thread, and the two subscription streams.
//!
//! Each call sends one request over the actor's `bounded(256)` request channel with a
//! `futures` oneshot for the reply. A reply that never comes (the host stopped or panicked) is
//! `internal`.

use std::path::PathBuf;
use std::thread::JoinHandle;

use clauseo_protocol::{
    CommandEnvelope, CommandOutcome, Cursor, FileRefId, FolderRefId, HostError, HostQuery,
    Principal, QueryResult, Seq, StreamId, ViewEpoch,
};
use futures::channel::oneshot;

use crate::actor::input::{Input, Request};
use crate::config::HostConfig;
use crate::error::Error;

mod subscription;

pub(crate) use subscription::Sink;
pub use subscription::{EventSubscription, Subscription};

/// The session host. [`Host::start`] runs it on its own thread, `clauseo-host`.
#[derive(Debug)]
#[non_exhaustive]
pub struct Host;

impl Host {
    /// Opens the store in `Effective::data_dir()` (created if missing), loads the matters and
    /// threads, creates the Scratch matter on the first start, repairs what the last run left
    /// open, and serves. Fails with `store.locked` when another host holds the data folder.
    pub fn start(config: HostConfig) -> Result<(HostHandle, HostJoin), HostError> {
        crate::actor::start(config).map_err(HostError::from)
    }
}

/// A caller's way to the host. Cheap to clone and `Send`; every clone reaches the same host.
#[derive(Debug, Clone)]
pub struct HostHandle {
    requests: async_channel::Sender<Request>,
    /// The actor's internal input channel, for `abort_for_test`.
    #[cfg(feature = "test-support")]
    internal: async_channel::Sender<Input>,
}

/// Waits for the host thread to end.
#[derive(Debug)]
pub struct HostJoin {
    thread: JoinHandle<Result<(), Error>>,
}

impl HostJoin {
    /// Wraps the host thread.
    pub(crate) fn new(thread: JoinHandle<Result<(), Error>>) -> Self {
        Self { thread }
    }

    /// Blocks until the host thread ends: `Ok` after a shutdown, `internal` when it panicked
    /// or failed.
    pub fn join(self) -> Result<(), HostError> {
        match self.thread.join() {
            Ok(result) => result.map_err(HostError::from),
            Err(_panic) => Err(HostError::from(Error::Closed)),
        }
    }
}

impl HostHandle {
    /// Wraps the actor's request channel and its internal input channel.
    pub(crate) fn new(
        requests: async_channel::Sender<Request>,
        internal: async_channel::Sender<Input>,
    ) -> Self {
        #[cfg(not(feature = "test-support"))]
        drop(internal);
        Self {
            requests,
            #[cfg(feature = "test-support")]
            internal,
        }
    }

    /// Sends one request and waits for its reply.
    async fn call<T>(
        &self,
        make: impl FnOnce(oneshot::Sender<Result<T, Error>>) -> Request,
    ) -> Result<T, HostError> {
        let (reply, answer) = oneshot::channel();
        self.requests
            .send(make(reply))
            .await
            .map_err(|_closed| HostError::from(Error::Closed))?;
        match answer.await {
            Ok(result) => result.map_err(HostError::from),
            Err(canceled) => Err(HostError::from(Error::from(canceled))),
        }
    }

    /// Runs a command as the local desktop: checks its shape, the receipt of its
    /// `command_id`, then the state, and answers once its events are durable.
    pub async fn dispatch(&self, command: CommandEnvelope) -> Result<CommandOutcome, HostError> {
        self.call(|reply| Request::Dispatch { command, reply })
            .await
    }

    /// Answers a query for `principal`.
    pub async fn query(
        &self,
        principal: Principal,
        query: HostQuery,
    ) -> Result<QueryResult, HostError> {
        self.call(|reply| Request::Query {
            principal,
            query,
            reply,
        })
        .await
    }

    /// Subscribes `principal` to the view of `stream`: the tail after `after` when `after`
    /// names the current epoch and a cursor still in the patch window, else one `Reset`; then
    /// live patches.
    pub async fn subscribe(
        &self,
        principal: Principal,
        stream: StreamId,
        after: Option<(ViewEpoch, Cursor)>,
    ) -> Result<Subscription, HostError> {
        self.call(|reply| Request::Subscribe {
            principal,
            stream,
            after,
            reply,
        })
        .await
    }

    /// Subscribes `principal` to the envelopes of `stream` after `after_seq` (from the first
    /// when `None`): the stored ones, one `CaughtUp`, then live ones as they become durable.
    pub async fn subscribe_events(
        &self,
        principal: Principal,
        stream: StreamId,
        after_seq: Option<Seq>,
    ) -> Result<EventSubscription, HostError> {
        self.call(|reply| Request::SubscribeEvents {
            principal,
            stream,
            after_seq,
            reply,
        })
        .await
    }

    /// Records that `principal` has applied the view of `stream` up to `cursor` of `epoch`.
    pub async fn ack(
        &self,
        principal: Principal,
        stream: StreamId,
        epoch: ViewEpoch,
        cursor: Cursor,
    ) -> Result<(), HostError> {
        self.call(|reply| Request::Ack {
            principal,
            stream,
            epoch,
            cursor,
            reply,
        })
        .await
    }

    /// A ref for a folder the lawyer picked, for `CreateMatter`. Kept in host memory only.
    pub async fn register_folder(&self, path: PathBuf) -> Result<FolderRefId, HostError> {
        self.call(|reply| Request::RegisterFolder { path, reply })
            .await
    }

    /// A ref for a file the lawyer picked, for a prompt. Kept in host memory only.
    pub async fn register_file(&self, path: PathBuf) -> Result<FileRefId, HostError> {
        self.call(|reply| Request::RegisterFile { path, reply })
            .await
    }

    /// Stops the host: no more commands, running turns cancelled, agents stopped, text
    /// flushed, the store closed. Answers once the store is closed; later calls are
    /// `internal`.
    pub async fn shutdown(&self) -> Result<(), HostError> {
        self.call(|reply| Request::Shutdown { reply }).await
    }

    /// Ends the host thread at once, with no flush and no graceful stop, as a crash would.
    /// Returns once the store is closed, so a new [`Host::start`] on the same folder works.
    #[cfg(feature = "test-support")]
    pub async fn abort_for_test(&self) {
        let (reply, done) = oneshot::channel();
        if self.internal.try_send(Input::Abort(reply)).is_ok() {
            // A dropped sender also means the thread is gone.
            let _gone = done.await;
        }
    }
}
