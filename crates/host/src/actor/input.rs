//! What reaches the actor: requests from handles over the bounded request channel, and
//! everything else (store results, matter and snapshot replies, agent events, timers, limits
//! reads, subscription asks) over the internal channel.

use std::path::PathBuf;

use clauseo_protocol::{
    AgentId, AgentKind, CommandEnvelope, CommandId, CommandOutcome, Cursor, Envelope, ErrorCode,
    FileRefId, FolderRefId, HostQuery, MatterId, Principal, QueryResult, SearchAnswer,
    SearchRequest, Seq, StreamId, ThreadId, ViewEpoch,
};
use clauseo_store::{Committed, Receipt, Reopen, SnapshotIn};
use futures::channel::oneshot;

use super::limits::LimitsInput;
use super::turn::{RestoreAsk, TurnInput, TurnJob, TurnTimer};
use crate::agent::AgentInput;
use crate::agent::login_link::AgentsStopped;
use crate::error::Error;
use crate::handle::{AppSubscription, EventSubscription, Subscription};
use crate::matter_worker::MatterReply;
use crate::pipeline::Ticket;

/// A reply to one handle call.
pub(crate) type Reply<T> = oneshot::Sender<Result<T, Error>>;

/// A sign-in call from a handle (decision record 0048).
#[derive(Debug)]
pub(crate) enum SignInRequest {
    /// `HostHandle::restart_agents`.
    Restart { agent: AgentId, reply: Reply<()> },
    /// `HostHandle::stop_agents`.
    Stop {
        agent: AgentId,
        reply: Reply<AgentsStopped>,
    },
    /// `HostHandle::set_reused_login`.
    Reused {
        agent: AgentId,
        reused: bool,
        reply: Reply<()>,
    },
    /// The folders whose login links `HostHandle::remove_login_links` removes.
    LinkHomes {
        agent: AgentId,
        reply: Reply<Vec<PathBuf>>,
    },
    /// `HostHandle::set_launch_refused`.
    LaunchRefused {
        agent: AgentId,
        code: Option<ErrorCode>,
        reply: Reply<()>,
    },
    /// `HostHandle::limits_login_changed`.
    LimitsLogin { agent: AgentKind, reply: Reply<()> },
}

/// How `SetPlan` writes `plans.<agent>`.
#[derive(Debug)]
pub(crate) enum PlanWrite {
    /// The whole entry, or `None` to remove it.
    Whole(Option<Box<clauseo_settings::Plan>>),
    /// The endpoint alone, or `None` to remove it, keeping the provider and the variables.
    Endpoint(Option<String>),
}

/// A subscription's number, unique for the host's run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct SubId(pub(crate) u64);

/// One call from a [`crate::HostHandle`].
#[derive(Debug)]
pub(crate) enum Request {
    Dispatch {
        command: CommandEnvelope,
        reply: Reply<CommandOutcome>,
    },
    Query {
        principal: Principal,
        query: HostQuery,
        reply: Reply<QueryResult>,
    },
    /// A search from `window`, superseded by a newer one from the same window.
    Search {
        principal: Principal,
        window: String,
        request: SearchRequest,
        reply: Reply<SearchAnswer>,
    },
    Subscribe {
        principal: Principal,
        stream: StreamId,
        after: Option<(ViewEpoch, Cursor)>,
        reply: Reply<Subscription>,
    },
    SubscribeEvents {
        principal: Principal,
        stream: StreamId,
        after_seq: Option<Seq>,
        reply: Reply<EventSubscription>,
    },
    SubscribeApp {
        principal: Principal,
        reply: Reply<AppSubscription>,
    },
    SubscribeLimits {
        principal: Principal,
        reply: Reply<Subscription>,
    },
    Ack {
        principal: Principal,
        stream: StreamId,
        epoch: ViewEpoch,
        cursor: Cursor,
        reply: Reply<()>,
    },
    RegisterFolder {
        path: PathBuf,
        reply: Reply<FolderRefId>,
    },
    RegisterFile {
        path: PathBuf,
        reply: Reply<FileRefId>,
    },
    LocateFile {
        principal: Principal,
        file: FileRefId,
        reply: Reply<crate::LocatedFile>,
    },
    MatterFolder {
        principal: Principal,
        matter: MatterId,
        reply: Reply<PathBuf>,
    },
    /// Desktop's write of an installed agent's entry (`actor/settings/entries.rs`).
    SetAgentLaunch {
        agent: AgentId,
        launch: Box<clauseo_settings::AgentLaunch>,
        reply: Reply<clauseo_settings::EntryWrite>,
    },
    /// Desktop's write of an agent's firm plan: the whole entry, or its endpoint alone.
    SetPlan {
        agent: AgentId,
        plan: PlanWrite,
        reply: Reply<()>,
    },
    /// A sign-in call (`turn/sign_in.rs`).
    SignIn(SignInRequest),
    Shutdown {
        reply: Reply<()>,
    },
}

/// What a matter job continues with when its reply comes back.
#[derive(Debug)]
pub(crate) enum MatterTag {
    /// The create or import of a `CreateMatter` command.
    Create(CommandId),
    /// The create of the Scratch matter at start-up.
    Scratch,
    /// The open the actor sends first to a matter's new worker; its reply is only logged.
    Open,
    /// The locate of a `RelocateMatter` command, with the folder it locates.
    Relocate {
        command: CommandId,
        folder: std::path::PathBuf,
    },
    /// A turn's job.
    Turn(TurnJob),
    /// A `RestoreVersion`'s restore.
    Restore(Box<RestoreAsk>),
    /// A query R8 answers: `CompareVersions`, `PreviewRestore`.
    Query(Reply<QueryResult>),
    /// A `ListPapers` query, answered with the listing.
    Papers(Reply<QueryResult>),
    /// The discard of a matter made on disk whose commit failed (`matters/made.rs`).
    Discard,
}

/// Everything else the actor reads, posted by its own tasks and by other threads.
pub(crate) enum Input {
    /// A commit's result, from the feeder's reply task.
    Committed {
        ticket: Ticket,
        result: Result<Committed, clauseo_store::Error>,
    },
    /// `Store::reopen` (or a whole-log read) for a stream being loaded.
    Reopened {
        stream: StreamId,
        result: Result<Reopen, clauseo_store::Error>,
    },
    /// The stored receipt of a command, if any.
    Receipt {
        command: CommandId,
        result: Result<Option<Receipt>, clauseo_store::Error>,
    },
    /// A page of stored envelopes for an event subscription that is catching up.
    EventPage {
        stream: StreamId,
        id: SubId,
        result: Result<Vec<Envelope>, clauseo_store::Error>,
    },
    /// Whether the store holds the row of a made matter whose commit failed (`matters/made.rs`).
    MadeRow {
        matter: MatterId,
        result: Result<bool, clauseo_store::Error>,
    },
    /// A matter worker's reply.
    Matter {
        matter: MatterId,
        tag: MatterTag,
        reply: MatterReply,
    },
    /// A snapshot compressed on the snapshot thread, to store.
    Snapshot(SnapshotIn),
    /// An agent's event or its end, for the thread it serves.
    Agent { thread: ThreadId, input: AgentInput },
    /// A turn timer fired.
    Timer(TurnTimer),
    /// The snooze timer armed with `generation` fired (`triage/wake.rs`).
    SnoozeWake { generation: u64 },
    /// A turn task's result: an agent call or a log read.
    Turn(TurnInput),
    /// A subscription drained its channel after the actor found it full.
    Drained { stream: StreamId, id: SubId },
    /// A subscription was dropped.
    Unsubscribed { stream: StreamId, id: SubId },
    /// An app subscription drained its channel after the actor found it full.
    AppDrained { id: SubId },
    /// An app subscription was dropped.
    AppUnsubscribed { id: SubId },
    /// A limits read ended, a limits timer fired, or a limits subscription drained or went.
    Limits(LimitsInput),
    /// What a `ListMatters` read of each matter's folder found, after it answered: the matter,
    /// the folder path its row had when the read began, and the state.
    FolderStates(Vec<(MatterId, String, clauseo_protocol::FolderState)>),
    /// A folder scan's thread or throttle timer.
    Scan(super::scan::ScanInput),
    /// Where the Chrome for Testing download stands, from its thread.
    BrowserInstall(clauseo_protocol::BrowserInstallState),
    /// A `SetSkillPack`'s read of its pack, from its thread (`settings/packs.rs`).
    SkillPack(Box<super::settings::packs::PackRead>),
    /// End the thread at once (tests); the sender is answered once the store is closed.
    #[cfg(feature = "test-support")]
    Abort(oneshot::Sender<()>),
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Committed { .. } => "Committed",
            Self::Reopened { .. } => "Reopened",
            Self::Receipt { .. } => "Receipt",
            Self::EventPage { .. } => "EventPage",
            Self::MadeRow { .. } => "MadeRow",
            Self::Matter { .. } => "Matter",
            Self::Snapshot(_) => "Snapshot",
            Self::Agent { .. } => "Agent",
            Self::Timer(_) => "Timer",
            Self::SnoozeWake { .. } => "SnoozeWake",
            Self::Turn(_) => "Turn",
            Self::Drained { .. } => "Drained",
            Self::Unsubscribed { .. } => "Unsubscribed",
            Self::AppDrained { .. } => "AppDrained",
            Self::AppUnsubscribed { .. } => "AppUnsubscribed",
            Self::Limits(_) => "Limits",
            Self::FolderStates(_) => "FolderStates",
            Self::Scan(_) => "Scan",
            Self::BrowserInstall(_) => "BrowserInstall",
            Self::SkillPack(_) => "SkillPack",
            #[cfg(feature = "test-support")]
            Self::Abort(_) => "Abort",
        };
        f.write_str(name)
    }
}
