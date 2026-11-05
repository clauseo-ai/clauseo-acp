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
    AgentId, AgentKind, CommandEnvelope, CommandOutcome, Cursor, ErrorCode, FileRefId, FolderRefId,
    HostError, HostQuery, MatterId, Principal, QueryResult, SearchAnswer, SearchRequest, Seq,
    StreamId, ViewEpoch,
};
use futures::channel::oneshot;

use crate::actor::input::{Input, PlanWrite, Request, SignInRequest};
use crate::agent::login_link;
use crate::agent::login_splits::{self, LoginSplits};
use crate::config::HostConfig;
use crate::error::Error;

mod subscription;

pub use subscription::{AppSubscription, EventSubscription, Subscription};
pub(crate) use subscription::{Front, SUBSCRIPTION_CAPACITY, Sink};

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
        let splits = config.login_splits();
        let (mut handle, join) = crate::actor::start(config).map_err(HostError::from)?;
        handle.splits = splits;
        Ok((handle, join))
    }
}

/// A caller's way to the host. Cheap to clone and `Send`; every clone reaches the same host.
#[derive(Debug, Clone)]
pub struct HostHandle {
    requests: async_channel::Sender<Request>,
    /// The login splits the actor sends (`HostHandle::login_splits`).
    splits: LoginSplits,
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
            // `Host::start` puts the config's own in.
            splits: login_splits::channel().1,
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

    /// Searches inside matters and conversations for `principal` (decision 0043). `window` names
    /// the requester: a newer search (a larger `generation`) from the same window answers this
    /// one `Cancelled`, interrupting it if it runs. `HostHandle::query` with `HostQuery::Search`
    /// is the same under one shared window.
    pub async fn search(
        &self,
        principal: Principal,
        window: &str,
        request: SearchRequest,
    ) -> Result<SearchAnswer, HostError> {
        let window = window.to_owned();
        self.call(|reply| Request::Search {
            principal,
            window,
            request,
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
    /// `transport.unauthorized`. The first frames are the current state; each subscriber has its
    /// own `bounded(1024)` channel; a full one gets the current state again once it has drained.
    /// Every subscription ends when the host stops.
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

    /// Writes `agents.<agent>` as `launch` (decision record 0048): desktop calls it when an
    /// agent's install is ready, Rust only, since a webview must not choose a program to run.
    /// The entry is written to `settings.json`, in force for the next launch, and every app
    /// subscriber gets `SettingsChanged`. Refused, with nothing written, with `settings.locked`
    /// for an id policy sets, and with `settings.refused` for a launch that is not Clauseo's
    /// own install (a command under `<data_dir>/agents/`, or `node` with its script there) or
    /// an `env` that sets a secret-shaped name; the message names the key, never a value. An
    /// entry already there that is the lawyer's or the firm's own is never replaced: the answer
    /// is [`EntryWrite::LeftAlone`](clauseo_settings::EntryWrite::LeftAlone), with nothing
    /// written or sent.
    pub async fn set_agent_launch(
        &self,
        agent: AgentId,
        launch: clauseo_settings::AgentLaunch,
    ) -> Result<clauseo_settings::EntryWrite, HostError> {
        let launch = Box::new(launch);
        self.call(|reply| Request::SetAgentLaunch {
            agent,
            launch,
            reply,
        })
        .await
    }

    /// Writes `plans.<agent>` as `plan`, or removes it with `None` (decision record 0048):
    /// desktop calls it, Rust only, after a firm endpoint's confirm. Written to `settings.json`,
    /// then `SettingsChanged` to every app subscriber. Refused, with nothing written, with
    /// `settings.locked` for a plan policy sets (every plan while a policy source cannot be read),
    /// and with `settings.refused` for an endpoint that is not an `https` address or a variable
    /// a plan may not set; the message names the key or the variable's place, never a value.
    pub async fn set_plan(
        &self,
        agent: AgentId,
        plan: Option<clauseo_settings::Plan>,
    ) -> Result<(), HostError> {
        let plan = PlanWrite::Whole(plan.map(Box::new));
        self.call(|reply| Request::SetPlan { agent, plan, reply })
            .await
    }

    /// Sets `plans.<agent>.endpoint` to `endpoint`, or removes it with `None`, keeping the
    /// plan's provider and variables (decision record 0048): desktop's `SetFirmEndpoint`, after
    /// its confirm. A plan left empty is removed. Read and written on the actor, so no other
    /// settings write falls between; refused as [`HostHandle::set_plan`] is.
    pub async fn set_plan_endpoint(
        &self,
        agent: AgentId,
        endpoint: Option<String>,
    ) -> Result<(), HostError> {
        let plan = PlanWrite::Endpoint(endpoint);
        self.call(|reply| Request::SetPlan { agent, plan, reply })
            .await
    }

    /// Lifts [`Self::stop_agents`]' hold on `agent`, stops every live agent of it that is not in
    /// a turn, so the next question launches with the new login, and resumes, once, each queue
    /// held for that agent's sign-in (a turn that failed with `agent.auth_required`, one
    /// [`Self::stop_agents`] cancelled, or a prompt sent while it held the agent). Rust
    /// only: desktop calls it on `SignedIn` and after a key, endpoint or source change. A
    /// second call resumes nothing more. A lawyer's `ResumeQueue` before it resumes the queue
    /// as usual, and the call then leaves it alone.
    pub async fn restart_agents(&self, agent: AgentId) -> Result<(), HostError> {
        self.call(|reply| Request::SignIn(SignInRequest::Restart { agent, reply }))
            .await
    }

    /// Step 1 of the leave order (decision record 0048, ruling 1): cancels each turn of
    /// `agent` through the Cancel path, puts it back at the head of its queue held for the
    /// sign-in, and stops every live agent of that id, launches being prepared included, so no
    /// process of it runs while the login link changes. Rust only.
    ///
    /// Answers once every process of the id has exited ([`AgentsStopped::Exited`]), or after
    /// the bounded wait (10 s, past the shutdown ladder's 7 s) or a process that outlived its
    /// kill ([`AgentsStopped::TimedOut`]); it never hangs. From then on the id is held, in
    /// memory only (a host restart clears it): nothing of it launches, and a prompt for it
    /// queues held for the sign-in, as #408 holds a queue. [`Self::restart_agents`] lifts the
    /// hold, relaunches and resumes once; desktop calls it too when its leave flow is
    /// abandoned.
    ///
    /// [`AgentsStopped::Exited`]: crate::AgentsStopped::Exited
    /// [`AgentsStopped::TimedOut`]: crate::AgentsStopped::TimedOut
    pub async fn stop_agents(&self, agent: AgentId) -> Result<crate::AgentsStopped, HostError> {
        self.call(|reply| Request::SignIn(SignInRequest::Stop { agent, reply }))
            .await
    }

    /// Desktop's word that `agent`'s login is the lawyer's own, reused (`true`, after
    /// `UseFoundLogin`'s link), or no longer (`false`). While it is, each Codex launch runs
    /// the split check through the [`LoginCheck`](crate::LoginCheck) hook. Rust only.
    pub async fn set_reused_login(&self, agent: AgentId, reused: bool) -> Result<(), HostError> {
        self.call(|reply| {
            Request::SignIn(SignInRequest::Reused {
                agent,
                reused,
                reply,
            })
        })
        .await
    }

    /// The reused Codex logins that split, as each launch's split check finds them: desktop
    /// reads them and runs S18's `stop_using_login` and the leave order. Rust only. The
    /// receiver keeps the latest split per agent, so a slow reader loses none and none back up
    /// (`LoginSplits`). Every clone takes from the same splits, so one reader takes each.
    pub fn login_splits(&self) -> LoginSplits {
        self.splits.clone()
    }

    /// Step 3 of the leave order: removes each matter's link to `agent`'s login file, without
    /// a restart. The actor only names the folders; the removal runs on a thread of its own with
    /// a bounded wait, so no file call blocks the actor. Matters get links in stage C (R17):
    /// until then there are none and nothing is removed. Rust only.
    pub async fn remove_login_links(&self, agent: AgentId) -> Result<(), HostError> {
        let asked = agent.clone();
        let homes = self
            .call(|reply| {
                Request::SignIn(SignInRequest::LinkHomes {
                    agent: asked,
                    reply,
                })
            })
            .await?;
        login_link::remove_links(agent, homes)
            .await
            .map_err(HostError::from)
    }

    /// Refuses `agent`'s new launches with `code` (decision record 0048, P4d), or, with
    /// `None`, lets them start again. A turn that would launch the agent fails as any refused
    /// launch does: an ordinary `TurnFailed` with `code`. Running sessions go on until they
    /// exit; desktop calls [`Self::stop_agents`] to end them sooner. Only the codes desktop may
    /// put there are taken, `signin.policy_unreadable` and `agent.keys_loading`: another is a
    /// bug, which debug builds assert, and release builds refuse with `command.invalid`,
    /// changing nothing. A launch meeting `agent.keys_loading` waits up to 2 s on the host
    /// clock for it to be lifted before it fails. In memory only. Rust only.
    pub async fn set_launch_refused(
        &self,
        agent: AgentId,
        code: Option<ErrorCode>,
    ) -> Result<(), HostError> {
        debug_assert!(
            code.is_none_or(crate::agent::launch_refusals::taken),
            "set_launch_refused takes no {code:?}"
        );
        self.call(|reply| Request::SignIn(SignInRequest::LaunchRefused { agent, code, reply }))
            .await
    }

    /// Desktop's word that `agent`'s login changed (a sign-in, a sign-out, an account change, or
    /// an API key or firm endpoint set or cleared; decision record 0048): `limits`'
    /// `Input::LoginChanged`, so the limits are read at once, past any age or back-off, and a
    /// read already in flight, which read the old login, is followed by another. The next
    /// scheduled read stays where it was. Rust only: no window can send it.
    pub async fn limits_login_changed(&self, agent: AgentKind) -> Result<(), HostError> {
        self.call(|reply| Request::SignIn(SignInRequest::LimitsLogin { agent, reply }))
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
