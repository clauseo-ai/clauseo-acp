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
    CommandId, CommandOutcome, FolderRefId, MatterId, Principal, StreamId, ThreadId,
};
use clauseo_settings::Effective;
use clauseo_store::Store;
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

mod app;
mod app_lists;
mod browser_install;
mod commands;
pub(crate) mod counting;
mod folder_state;
pub(crate) mod header_tools;
pub(crate) mod input;
pub(crate) mod limits;
mod locate;
mod matter_details;
mod matters;
mod papers;
mod query;
mod rail;
mod relocate;
pub(crate) mod scan;
mod seen;
mod settings;
mod shutdown;
mod startup;
mod state;
mod streams;
mod subs;
mod title_source;
mod triage;
pub(crate) mod turn;
mod update;

use commands::PendingCommand;
use input::{Input, MatterTag, Request};
pub(crate) use state::{HostPhase, MatterEntry, Tag, ThreadEntry, Work};
use streams::StreamSlot;
use subs::EventSub;

/// The name of the host thread.
const THREAD_NAME: &str = "clauseo-host";

/// The capacity of the request channel from handles.
const REQUEST_CAPACITY: usize = 256;

/// How the loop ended.
enum Exit {
    Shutdown(Vec<input::Reply<()>>),
    #[cfg(feature = "test-support")]
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
    /// The search thread (`search.rs`): each returned commit goes to it, and searches.
    pub(crate) search: crate::search::Search,
    /// The settings. Only the actor touches them, so it is the one lock their writes need: each
    /// `Set…Settings` runs its `Effective` write here, one command at a time.
    pub(crate) effective: Effective,
    /// The policy lock causes last logged (`settings/lock_causes.rs`): a reload logs a change.
    pub(crate) lock_causes: Vec<clauseo_settings::LockReason>,
    /// The usage counts (`HostConfig::with_counts`), or none: every call site is then a no-op
    /// (`counting.rs`).
    pub(crate) counts: Option<clauseo_counts::Counts>,
    /// What to count once a command's commit is durable, by command (`counting.rs`).
    pub(crate) count_on_commit: BTreeMap<clauseo_protocol::CommandId, counting::OnCommit>,
    pub(crate) launcher: Arc<dyn Launcher>,
    /// Called at each session start, for the `_meta` of `session/new` and `session/load`, and
    /// at each launch, for the agent's variables (`agent/loads.rs`).
    pub(crate) profile: Box<dyn ProfileHook>,
    /// Where Claude Code's enterprise `managed-mcp.json` would be.
    pub(crate) managed_mcp: std::path::PathBuf,
    /// The Codex hooks object a test set (`HostConfig::with_codex_hooks`); read only through
    /// the codex hooks seam, `turn::agents::codex_hooks`.
    pub(crate) codex_hooks: Option<serde_json::Map<String, serde_json::Value>>,
    /// Where each launch's folder check answers from (`agent/folder_support.rs`).
    pub(crate) folder_check: crate::agent::folder_support::CheckSource,
    /// The program the pre-write guard hook runs, if any (`HostConfig::with_pre_write_guard`).
    pub(crate) pre_write_guard: Option<std::path::PathBuf>,
    /// The proxy, stetdoc's folder and the browser inputs (`HostConfig::with_egress_proxy`).
    pub(crate) wiring: crate::config::LaunchWiring,
    /// The Chrome for Testing download: its tools, its start-up sweep and the one running.
    pub(crate) browser_install: browser_install::BrowserInstall,
    /// The tools each thread's header shows, laid over the header unlogged.
    pub(crate) header_tools: header_tools::HeaderTools,
    /// The start-up sweep of leftover `run/` folders, done by the first launch's prep.
    pub(crate) run_sweep: turn::RunSweep,
    pub(crate) refused_folders: Vec<PathBuf>,
    /// The lawyer's home folder as configured, for `matter.folder_is_home` (the refused list
    /// names it the same way).
    pub(crate) home: Option<PathBuf>,
    /// The home folder's spellings for `folder_display`: as configured and canonicalised once
    /// at start-up (`rail::home_spellings`).
    pub(crate) home_spellings: Vec<PathBuf>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) ids: Ids,
    pub(crate) window_cap: usize,
    /// The capacity of each app subscription's channel.
    pub(crate) app_capacity: usize,
    pub(crate) encoder: Encoder,
    pub(crate) matters: BTreeMap<MatterId, MatterEntry>,
    /// Matters `ForgetMatter` took off the list: inert (no command, query or start-up repair
    /// reaches them) until an import of their folder brings them back. Their threads stay in
    /// `threads`.
    pub(crate) forgotten: BTreeMap<MatterId, MatterEntry>,
    /// The time of each stream's last durable event, for `MatterSummary::last_activity`: read
    /// once at start-up (`Store::tail_times`), then kept by every returned commit.
    pub(crate) tails: BTreeMap<StreamId, clauseo_protocol::Timestamp>,
    pub(crate) threads: BTreeMap<ThreadId, ThreadEntry>,
    pub(crate) streams: BTreeMap<StreamId, StreamSlot>,
    pub(crate) event_subs: BTreeMap<StreamId, Vec<EventSub>>,
    /// The app subscribers, in the order they came.
    pub(crate) app_subs: Vec<app::AppSub>,
    /// The limits model, its reader, timer and subscribers (`limits.rs`).
    pub(crate) limits: limits::LimitsHost,
    /// The rail's triage: streams it holds loaded and the snooze timer (`triage.rs`).
    pub(crate) triage: triage::Triage,
    /// The matter and thread rows last sent to the app subscribers, and what changed since.
    pub(crate) lists: app_lists::Lists,
    pub(crate) commands: BTreeMap<CommandId, PendingCommand>,
    pub(crate) folders: BTreeMap<FolderRefId, PathBuf>,
    /// Folders a command is making a matter of (a create, an import, a relocate), by command,
    /// until it is answered: the registry check counts them as the matters they will be.
    pub(crate) claims: BTreeMap<CommandId, (MatterId, PathBuf)>,
    /// Each matter's folder state as the last `ListMatters` read it, for the rows sent to app
    /// subscribers (`app_lists.rs`): in memory only, never logged. A matter with none reads
    /// `found`; a relocate and a forget drop its entry.
    pub(crate) folder_states: BTreeMap<MatterId, clauseo_protocol::FolderState>,
    /// Each matter's last folder check, for its row; in memory only. None: fine or unchecked.
    pub(crate) folder_supports: BTreeMap<MatterId, clauseo_protocol::FolderSupportKind>,
    /// The folder scans whose thread has not ended (`scan.rs`).
    pub(crate) scans: scan::Scans,
    /// The walk each scan runs: `clauseo_matters::scan_folder`, or a test's stand-in.
    pub(crate) scan_walk: scan::Walk,
    /// Matter folders whose `ListMatters` stat is still running (`folder_state.rs`).
    pub(crate) stats_running: folder_state::Running,
    pub(crate) files: papers::FileRefs,
    pub(crate) next_sub: u64,
    pub(crate) work: Work,
    pub(crate) phase: HostPhase,
    /// The Scratch matter being created at start-up.
    pub(crate) scratch: Option<Box<matters::PendingMatter>>,
    /// The workers of matters made on disk whose commit has not returned, and of those being
    /// discarded because it failed (`matters/made.rs`), by matter.
    pub(crate) made: BTreeMap<MatterId, matters::Made>,
    /// Picks commits for the store to refuse (tests): given a commit's events, a command id the
    /// store already holds. The commit then carries a receipt with it: `DuplicateCommand`.
    #[cfg(test)]
    pub(crate) fault: Option<Fault>,
    /// How many of the next stored commits are reported as failed (`Closed`) though stored
    /// (tests): a reply lost on its way back.
    #[cfg(test)]
    pub(crate) lost_replies: usize,
    /// How many of the next commits fail to send (tests), as with the store's feeder gone.
    #[cfg(test)]
    pub(crate) failed_sends: usize,
}

/// See [`Actor::fault`].
#[cfg(test)]
pub(crate) type Fault = Box<dyn FnMut(&[clauseo_protocol::Envelope]) -> Option<CommandId>>;

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
    let ended: Result<_, futures::channel::oneshot::Sender<()>> = pool.run_until(async move {
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
            #[cfg(feature = "test-support")]
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
            Some(Next::Request(request)) => {
                actor.on_request(request);
                triage::tick(actor);
                app_lists::flush(actor);
            }
            // Every handle is gone (the internal channel never ends: the actor holds a sender).
            Some(Next::HandlesGone) | None => shutdown::begin_shutdown(actor, None),
            #[cfg(feature = "test-support")]
            Some(Next::Input(Input::Abort(reply))) => return Exit::Abort(reply),
            Some(Next::Input(input)) => {
                actor.on_input(input);
                triage::tick(actor);
                app_lists::flush(actor);
            }
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
        self.post_after(delay, Input::Timer(timer))
    }

    /// Posts `input` after `delay` on the host clock. Abort the handle to disarm it.
    pub(crate) fn post_after(&self, delay: Duration, input: Input) -> AbortHandle {
        let (handle, registration) = AbortHandle::new_pair();
        let after = self.clock.after(delay);
        let post = self.post.clone();
        let task = Abortable::new(
            async move {
                after.await;
                let _gone = post.try_send(input);
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
        #[cfg(test)]
        if self.failed_sends > 0 {
            self.failed_sends -= 1;
            return Err(Error::Closed);
        }
        #[cfg(test)]
        let draft = self.with_fault(draft);
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
        if !matches!(self.phase, HostPhase::Serving) {
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
            Request::Search {
                principal,
                window,
                request,
                reply,
            } => match authorize(&principal, Access::Query) {
                Ok(()) => self.search.query(
                    &window,
                    request,
                    Box::new(move |answer| {
                        let _gone = reply.send(answer);
                    }),
                ),
                Err(error) => {
                    let _gone = reply.send(Err(error));
                }
            },
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
            Request::SubscribeApp { principal, reply } => app::subscribe(self, &principal, reply),
            Request::SubscribeLimits { principal, reply } => {
                limits::subscribe(self, &principal, reply);
            }
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
                    .and_then(|()| papers::file_ref(self, path));
                let _gone = reply.send(minted);
            }
            Request::LocateFile {
                principal,
                file,
                reply,
            } => {
                let located = authorize(&principal, Access::LocateFile)
                    .and_then(|()| locate::locate_file(self, file));
                let _gone = reply.send(located);
            }
            Request::MatterFolder {
                principal,
                matter,
                reply,
            } => {
                let folder = authorize(&principal, Access::MatterFolder)
                    .and_then(|()| locate::matter_folder(self, matter));
                let _gone = reply.send(folder);
            }
            Request::SetAgentLaunch {
                agent,
                launch,
                reply,
            } => {
                let written = settings::set_agent_launch(self, &agent, &launch);
                let _gone = reply.send(written);
            }
            Request::SetPlan { agent, plan, reply } => {
                let written = settings::set_plan(self, &agent, plan.as_deref());
                let _gone = reply.send(written);
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
            Input::MadeRow { matter, result } => matters::row_checked(self, matter, result),
            Input::Matter { matter, tag, reply } => {
                matters::on_matter_reply(self, matter, tag, reply);
            }
            Input::Snapshot(snapshot) => streams::on_snapshot(self, snapshot),
            Input::Agent { thread, input } => turn::agent_input(self, thread, input),
            Input::Timer(timer) => turn::timer(self, timer),
            Input::Turn(input) => turn::turn_input(self, input),
            Input::Drained { stream, id } => subs::on_drained(self, stream, id),
            Input::Unsubscribed { stream, id } => subs::on_unsubscribed(self, stream, id),
            Input::AppDrained { id } => app::on_drained(self, id),
            Input::AppUnsubscribed { id } => app::on_unsubscribed(self, id),
            Input::Limits(input) => limits::on_input(self, input),
            Input::SnoozeWake { generation } => triage::on_wake(self, generation),
            Input::FolderStates(states) => query::on_folder_states(self, states),
            Input::Scan(input) => scan::on_input(self, input),
            Input::BrowserInstall(state) => browser_install::on_state(self, state),
            // The loop handles an abort before it gets here.
            #[cfg(feature = "test-support")]
            Input::Abort(reply) => {
                let _gone = reply.send(());
            }
        }
    }

    /// Moves start-up to serving, and shutdown to its end, once their work is done.
    fn settle(&mut self) -> Option<Exit> {
        if !self.work.idle() || turn::busy(self) || browser_install::busy(self) {
            return None;
        }
        match std::mem::replace(&mut self.phase, HostPhase::Serving) {
            HostPhase::Starting(ready) => {
                if self.scratch.is_some() {
                    self.phase = HostPhase::Starting(ready);
                    return None;
                }
                // `start` may have given up waiting; then nobody is listening.
                let _gone = ready.send(Ok(()));
                counting::app_opened(self);
                None
            }
            HostPhase::Serving => None,
            HostPhase::Stopping(replies) => Some(Exit::Shutdown(replies)),
        }
    }
}

#[cfg(test)]
mod tests;
