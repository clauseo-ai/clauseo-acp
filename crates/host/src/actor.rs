//! The host thread, `clauseo-host`: one actor on a `futures` `LocalPool`.
//!
//! `main` owns every piece of state and loops over one merged input stream: the requests of
//! handles (`bounded(256)`) and the internal channel, where the actor's own tasks and the
//! matter and snapshot threads post store results, matter replies, snapshots, agent events,
//! timer fires and subscription asks. It awaits nothing else: anything that waits runs as a
//! spawned task that posts its result back. Replies go out over `futures` oneshots.

use std::collections::BTreeMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::pin;
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::Duration;

use clauseo_process::Launcher;
use clauseo_protocol::{
    CommandId, CommandOutcome, FileRefId, FolderRefId, MatterId, Principal, StreamId, ThreadId,
};
use clauseo_settings::Effective;
use clauseo_store::{MatterRow, Store, ThreadRow};
use futures::executor::{LocalPool, LocalSpawner};
use futures::future::{AbortHandle, Abortable};
use futures::task::LocalSpawnExt;
use futures::{StreamExt, stream};

use crate::clock::{Clock, Now};
use crate::config::{HostConfig, ProfileHook};
use crate::encode::Encoder;
use crate::error::Error;
use crate::handle::{HostHandle, HostJoin};
use crate::ids::Ids;
use crate::matter_worker::MatterWorker;
use crate::pipeline::{Draft, FeederTx, Pipeline, Ticket};
use crate::principal::{Access, authorize};
use crate::snapshot::SnapshotWorker;

mod commands;
pub(crate) mod input;
mod matters;
mod query;
mod shutdown;
mod startup;
mod streams;
mod subs;
pub(crate) mod turn;

use commands::{AfterCommand, PendingCommand};
use input::{Input, MatterTag, Request};
use streams::StreamSlot;
use subs::EventSub;

/// The name of the host thread.
const THREAD_NAME: &str = "clauseo-host";

/// The capacity of the request channel from handles.
const REQUEST_CAPACITY: usize = 256;

/// What a commit continues with once its result is back.
pub(crate) enum Tag {
    /// A command's commit: answer `outcome` (or the failure) after `after`.
    Command {
        id: CommandId,
        outcome: CommandOutcome,
        after: AfterCommand,
    },
    /// The Scratch matter's commit at start-up.
    Scratch(Box<MatterEntry>),
    /// A snapshot with no events.
    Snapshot,
    /// A turn's commit.
    Turn(turn::TurnTag),
}

/// A matter the host knows.
pub(crate) struct MatterEntry {
    pub(crate) row: MatterRow,
    /// Its thread, started (and the matter opened) on first use.
    pub(crate) worker: Option<MatterWorker<MatterTag>>,
    pub(crate) turns: turn::MatterTurns,
}

/// A thread the host knows.
#[derive(Debug)]
pub(crate) struct ThreadEntry {
    pub(crate) row: ThreadRow,
    /// The header as of the last time the stream was unloaded, for `ListThreads`.
    pub(crate) header: Option<clauseo_protocol::HeaderEntry>,
    pub(crate) turn: turn::TurnState,
}

/// Work in flight that start-up and shutdown wait for.
#[derive(Debug, Default)]
pub(crate) struct Work {
    pub(crate) commits: usize,
    pub(crate) loads: usize,
    pub(crate) receipts: usize,
    pub(crate) matter_jobs: usize,
    pub(crate) snapshots: usize,
}

impl Work {
    fn idle(&self) -> bool {
        self.commits == 0
            && self.loads == 0
            && self.receipts == 0
            && self.matter_jobs == 0
            && self.snapshots == 0
    }
}

/// Where the host is in its life.
#[derive(Debug)]
pub(crate) enum Phase {
    /// Start-up work runs; `ready` hears once it is done.
    Starting(mpsc::Sender<Result<(), Error>>),
    Serving,
    /// Shutdown runs; every reply hears once the store is closed.
    Stopping(Vec<input::Reply<()>>),
}

/// How the loop ended.
enum Exit {
    Shutdown(Vec<input::Reply<()>>),
    Abort(futures::channel::oneshot::Sender<()>),
}

/// The actor's state. Everything lives on the host thread.
pub(crate) struct Actor {
    pub(crate) spawner: LocalSpawner,
    /// The internal input channel, for tasks and threads that post back.
    pub(crate) post: async_channel::Sender<Input>,
    pub(crate) store: Store,
    pub(crate) pipeline: Pipeline<Tag>,
    pub(crate) feeder: FeederTx,
    pub(crate) snapshots: SnapshotWorker,
    pub(crate) effective: Arc<Effective>,
    pub(crate) launcher: Arc<dyn Launcher>,
    #[expect(dead_code, reason = "calling the profile hook is a later follow-up")]
    pub(crate) profile: Box<dyn ProfileHook>,
    pub(crate) refused_folders: Vec<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Ids,
    pub(crate) window_cap: usize,
    pub(crate) encoder: Encoder,
    pub(crate) matters: BTreeMap<MatterId, MatterEntry>,
    pub(crate) threads: BTreeMap<ThreadId, ThreadEntry>,
    pub(crate) streams: BTreeMap<StreamId, StreamSlot>,
    pub(crate) event_subs: BTreeMap<StreamId, Vec<EventSub>>,
    pub(crate) commands: BTreeMap<CommandId, PendingCommand>,
    pub(crate) folders: BTreeMap<FolderRefId, PathBuf>,
    pub(crate) files: BTreeMap<FileRefId, PathBuf>,
    pub(crate) next_sub: u64,
    pub(crate) work: Work,
    pub(crate) phase: Phase,
    /// The Scratch matter being created at start-up.
    pub(crate) scratch: Option<Box<matters::PendingMatter>>,
}

/// Starts the host thread and returns once it serves.
pub(crate) fn start(config: HostConfig) -> Result<(HostHandle, HostJoin), Error> {
    let (requests, request_rx) = async_channel::bounded::<Request>(REQUEST_CAPACITY);
    let (internal, internal_rx) = async_channel::unbounded::<Input>();
    let (ready, started) = mpsc::channel::<Result<(), Error>>();
    let post = internal.clone();
    let thread = thread::Builder::new()
        .name(THREAD_NAME.to_owned())
        .spawn(move || run(config, request_rx, post, internal_rx, ready))
        .map_err(|error| Error::Internal(format!("starting {THREAD_NAME}: {error}")))?;
    match started.recv() {
        Ok(Ok(())) => Ok((HostHandle::new(requests, internal), HostJoin::new(thread))),
        Ok(Err(error)) => {
            // The thread closes what it opened and ends; its own result adds nothing.
            let _ended = thread.join();
            Err(error)
        }
        Err(_gone) => match thread.join() {
            Ok(Err(error)) => Err(error),
            Ok(Ok(())) | Err(_) => Err(Error::Internal(format!(
                "{THREAD_NAME} ended while starting"
            ))),
        },
    }
}

/// The host thread's body.
fn run(
    config: HostConfig,
    requests: async_channel::Receiver<Request>,
    post: async_channel::Sender<Input>,
    internal: async_channel::Receiver<Input>,
    ready: mpsc::Sender<Result<(), Error>>,
) -> Result<(), Error> {
    let mut pool = LocalPool::new();
    let spawner = pool.spawner();
    let ended = pool.run_until(async move {
        let mut actor = match startup::open(config, spawner, post, ready.clone()).await {
            Ok(actor) => actor,
            Err(error) => {
                let _gone = ready.send(Err(error));
                return Ok(None);
            }
        };
        startup::begin_startup(&mut actor);
        let exit = main_loop(&mut actor, requests, internal).await;
        match exit {
            Exit::Shutdown(replies) => {
                let closed = shutdown::close(actor).await;
                Ok(Some((closed, replies)))
            }
            Exit::Abort(reply) => {
                drop(actor);
                Err(reply)
            }
        }
    });
    // Dropping the pool drops every task, and with them the last clones of the store.
    drop(pool);
    match ended {
        Ok(None) => Ok(()),
        Ok(Some((closed, replies))) => shutdown::answer(closed, replies),
        Err(abort_reply) => {
            let _gone = abort_reply.send(());
            Ok(())
        }
    }
}

/// One item of the merged input stream.
enum Next {
    Request(Request),
    /// Every handle is gone: shut down.
    HandlesGone,
    Input(Input),
}

/// The loop: one input at a time until shutdown or abort.
async fn main_loop(
    actor: &mut Actor,
    requests: async_channel::Receiver<Request>,
    internal: async_channel::Receiver<Input>,
) -> Exit {
    let requests = requests
        .map(Next::Request)
        .chain(stream::once(async { Next::HandlesGone }));
    let mut inputs = pin!(stream::select(requests, internal.map(Next::Input)));
    loop {
        if let Some(exit) = actor.settle() {
            return exit;
        }
        match inputs.next().await {
            Some(Next::Request(request)) => actor.on_request(request),
            // Every handle is gone (the internal channel never ends: the actor holds a sender).
            Some(Next::HandlesGone) | None => shutdown::begin_shutdown(actor, None),
            Some(Next::Input(Input::Abort(reply))) => return Exit::Abort(reply),
            Some(Next::Input(input)) => actor.on_input(input),
        }
    }
}

impl Actor {
    /// Runs `then` once `stream` is loaded (at once when it is), loading it first.
    pub(crate) fn with_loaded(&mut self, stream: StreamId, then: streams::Waiter) {
        streams::with_loaded(self, stream, then);
    }

    /// The loaded state, view and sent view of `stream`, if it is loaded.
    pub(crate) fn loaded(&self, stream: StreamId) -> Option<&streams::Loaded> {
        streams::loaded(self, stream)
    }

    /// Unloads `stream` when nothing needs it any more.
    pub(crate) fn release(&mut self, stream: StreamId) {
        streams::release(self, stream);
    }

    /// Answers every caller waiting on command `id` and forgets it.
    pub(crate) fn finish_command(&mut self, id: CommandId, result: Result<CommandOutcome, Error>) {
        commands::finish_command(self, id, result);
    }

    /// The thread `thread`, or `command.unknown_thread`.
    pub(crate) fn check_thread(&self, thread: ThreadId) -> Result<&ThreadEntry, Error> {
        commands::check_thread(self, thread)
    }

    /// The matter `matter`, or `command.unknown_matter`.
    pub(crate) fn check_matter(&self, matter: MatterId) -> Result<&MatterEntry, Error> {
        commands::check_matter(self, matter)
    }

    /// Whether the host knows `stream` (`command.unknown_thread` or `command.unknown_matter`).
    pub(crate) fn check_stream(&self, stream: StreamId) -> Result<(), Error> {
        subs::check_stream(self, stream)
    }

    /// The worker of a known matter, started and sent its open on first use.
    pub(crate) fn matter_worker(
        &mut self,
        matter: MatterId,
    ) -> Result<&MatterWorker<MatterTag>, Error> {
        matters::matter_worker(self, matter)
    }

    /// The host clock's time.
    pub(crate) fn now(&self) -> Now {
        self.clock.now()
    }

    /// Runs `task` on the pool and posts its result to the internal channel.
    pub(crate) fn spawn_post(&self, task: impl Future<Output = Input> + 'static) {
        let post = self.post.clone();
        let spawned = self.spawner.spawn_local(async move {
            // The actor may be gone by then; nobody is left to tell.
            let _gone = post.try_send(task.await);
        });
        if let Err(error) = spawned {
            tracing::error!(%error, "the host pool refused a task");
        }
    }

    /// Posts `timer` after `delay` on the host clock. Abort the handle to disarm it.
    pub(crate) fn arm_timer(&self, delay: Duration, timer: turn::TurnTimer) -> AbortHandle {
        let (handle, registration) = AbortHandle::new_pair();
        let after = self.clock.after(delay);
        let post = self.post.clone();
        let task = Abortable::new(
            async move {
                after.await;
                let _gone = post.try_send(Input::Timer(timer));
            },
            registration,
        );
        if let Err(error) = self.spawner.spawn_local(async move {
            let _aborted = task.await;
        }) {
            tracing::error!(%error, "the host pool refused a timer");
        }
        handle
    }

    /// Sends `draft` with `tag` and folds its events into the sent views of loaded streams.
    pub(crate) fn send_commit(&mut self, draft: Draft, tag: Tag) -> Result<Ticket, Error> {
        let ticket = self.pipeline.send(draft, tag, &self.feeder)?;
        self.work.commits += 1;
        let events = self.pipeline.events(ticket);
        let mut seen: Vec<StreamId> = Vec::new();
        for envelope in events {
            if seen.contains(&envelope.stream) {
                continue;
            }
            seen.push(envelope.stream);
            if let Some(StreamSlot::Loaded(loaded)) = self.streams.get_mut(&envelope.stream) {
                loaded.sent.on_sent(events);
            }
        }
        Ok(ticket)
    }

    /// One request from a handle.
    fn on_request(&mut self, request: Request) {
        if !matches!(self.phase, Phase::Serving) {
            if let Request::Shutdown { reply } = request {
                shutdown::begin_shutdown(self, Some(reply));
            } else {
                // Dropping the reply answers `internal`: the host is stopping.
                drop(request);
            }
            return;
        }
        match request {
            Request::Dispatch { command, reply } => commands::dispatch(self, command, reply),
            Request::Query {
                principal,
                query,
                reply,
            } => query::query(self, &principal, &query, reply),
            Request::Subscribe {
                principal,
                stream,
                after,
                reply,
            } => subs::subscribe(self, &principal, stream, after, reply),
            Request::SubscribeEvents {
                principal,
                stream,
                after_seq,
                reply,
            } => subs::subscribe_events(self, &principal, stream, after_seq, reply),
            Request::Ack {
                principal,
                stream,
                epoch,
                cursor,
                reply,
            } => {
                let acked = authorize(&principal, Access::Ack).map(|()| {
                    subs::ack(self, stream, epoch, cursor);
                });
                let _gone = reply.send(acked);
            }
            Request::RegisterFolder { path, reply } => {
                let minted = authorize(&Principal::LocalDesktop, Access::RegisterFolder)
                    .and_then(|()| self.ids.mint::<FolderRefId>(self.clock.now().wall));
                let _gone = reply.send(minted.inspect(|id| {
                    self.folders.insert(*id, path);
                }));
            }
            Request::RegisterFile { path, reply } => {
                let minted = authorize(&Principal::LocalDesktop, Access::RegisterFile)
                    .and_then(|()| self.ids.mint::<FileRefId>(self.clock.now().wall));
                let _gone = reply.send(minted.inspect(|id| {
                    self.files.insert(*id, path);
                }));
            }
            Request::Shutdown { reply } => {
                match authorize(&Principal::LocalDesktop, Access::Shutdown) {
                    Ok(()) => shutdown::begin_shutdown(self, Some(reply)),
                    Err(error) => {
                        let _gone = reply.send(Err(error));
                    }
                }
            }
        }
    }

    /// One internal input.
    fn on_input(&mut self, input: Input) {
        match input {
            Input::Committed { ticket, result } => streams::on_committed(self, ticket, result),
            Input::Reopened { stream, result } => streams::on_reopened(self, stream, result),
            Input::Receipt { command, result } => commands::on_receipt(self, command, result),
            Input::EventPage { stream, id, result } => {
                subs::on_event_page(self, stream, id, result);
            }
            Input::Matter { matter, tag, reply } => {
                matters::on_matter_reply(self, matter, tag, reply);
            }
            Input::Snapshot(snapshot) => streams::on_snapshot(self, snapshot),
            Input::Agent { thread, input } => turn::agent_input(self, thread, input),
            Input::Timer(timer) => turn::timer(self, timer),
            Input::Turn(input) => turn::turn_input(self, input),
            Input::Drained { stream, id } => subs::on_drained(self, stream, id),
            Input::Unsubscribed { stream, id } => subs::on_unsubscribed(self, stream, id),
            // The loop handles an abort before it gets here.
            Input::Abort(reply) => {
                let _gone = reply.send(());
            }
        }
    }

    /// Moves start-up to serving, and shutdown to its end, once their work is done.
    fn settle(&mut self) -> Option<Exit> {
        if !self.work.idle() || turn::busy(self) {
            return None;
        }
        match std::mem::replace(&mut self.phase, Phase::Serving) {
            Phase::Starting(ready) => {
                if self.scratch.is_some() {
                    self.phase = Phase::Starting(ready);
                    return None;
                }
                // `start` may have given up waiting; then nobody is listening.
                let _gone = ready.send(Ok(()));
                None
            }
            Phase::Serving => None,
            Phase::Stopping(replies) => Some(Exit::Shutdown(replies)),
        }
    }
}

#[cfg(test)]
mod tests;
