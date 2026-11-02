//! The host's public surface: [`Host::start`], the [`HostHandle`] every caller holds, the
//! [`HostJoin`] that waits for the host thread, and the three subscription streams (a limits
//! subscription is a [`Subscription`] too).
//!
//! Each call sends one request over the actor's `bounded(256)` request channel with a
//! `futures` oneshot for the reply. A reply that never comes (the host stopped or panicked) is
//! `internal`.

use std::path::PathBuf;
use std::thread::JoinHandle;

use clauseo_protocol::{
    CommandEnvelope, CommandOutcome, Cursor, FileRefId, FolderRefId, HostError, HostQuery,
    MatterId, Principal, QueryResult, Seq, StreamId, ViewEpoch,
};
use futures::channel::oneshot;

use crate::actor::input::{Input, Request};
use crate::config::HostConfig;
use crate::error::Error;

mod subscription;

pub use subscription::{AppSubscription, EventSubscription, Subscription};
pub(crate) use subscription::{SUBSCRIPTION_CAPACITY, Sink};

/// A file a ref names, as [`HostHandle::locate_file`] found it: the matter that owns it, that
/// matter's folder, and the file, as the host recorded them (not resolved).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocatedFile {
    /// The matter whose folder holds the file.
    pub matter: MatterId,
    /// That matter's folder.
    pub folder: PathBuf,
    /// The file.
    pub path: PathBuf,
}

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

    /// Subscribes `principal` to the app-wide events, those of no stream (see
    /// [`AppSubscription`] for what comes and the rules). It checks the principal as
    /// [`HostHandle::subscribe`] does: one this version does not know is
    /// `transport.unauthorized`. Each subscriber has its own `bounded(1024)` channel; a full
    /// one gets the current state once it has drained. Every subscription ends when the host
    /// stops.
    pub async fn subscribe_app(&self, principal: Principal) -> Result<AppSubscription, HostError> {
        self.call(|reply| Request::SubscribeApp { principal, reply })
            .await
    }

    /// Subscribes `principal` to the limits of every agent account (see [`Subscription`] for
    /// what comes): first the current `LimitsViewChanged`, then for each change one
    /// `AccountLimitsChanged` per changed account and one `LimitsViewChanged`. It checks the
    /// principal as [`HostHandle::subscribe`] does. While any limits subscriber is left the host
    /// runs the limits deadlines (stale marks, reset times); with none it arms no limits timer.
    /// A full subscriber gets the current view once it has drained. Every subscription ends
    /// when the host stops.
    pub async fn subscribe_limits(&self, principal: Principal) -> Result<Subscription, HostError> {
        self.call(|reply| Request::SubscribeLimits { principal, reply })
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

    /// A ref for a file the lawyer picked, for a prompt. Kept in host memory only. The same
    /// path (as written) gets the same ref for the host's run, the one `ListPapers` gives it.
    pub async fn register_file(&self, path: PathBuf) -> Result<FileRefId, HostError> {
        self.call(|reply| Request::RegisterFile { path, reply })
            .await
    }

    /// What `file` names, for the desktop's `open_file` (0026): the matter that owns it, that
    /// matter's folder, and the file, checked now against that folder with symlinks and `..`
    /// followed. Refused with `command.invalid` for a ref this run did not issue, and for a file
    /// in no matter's folder or outside its owner's; `matter.folder_missing` when the folder is
    /// gone. Only the local desktop may ask.
    pub async fn locate_file(
        &self,
        principal: Principal,
        file: FileRefId,
    ) -> Result<LocatedFile, HostError> {
        self.call(|reply| Request::LocateFile {
            principal,
            file,
            reply,
        })
        .await
    }

    /// The folder recorded for `matter`, for the desktop's `reveal` (0026):
    /// `command.unknown_matter` for a matter the host does not know. Only the local desktop may
    /// ask.
    pub async fn matter_folder(
        &self,
        principal: Principal,
        matter: MatterId,
    ) -> Result<PathBuf, HostError> {
        self.call(|reply| Request::MatterFolder {
            principal,
            matter,
            reply,
        })
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
