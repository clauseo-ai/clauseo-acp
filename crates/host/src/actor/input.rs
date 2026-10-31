//! What reaches the actor: requests from handles over the bounded request channel, and
//! everything else (store results, matter and snapshot replies, agent events, timers,
//! subscription asks) over the internal channel.

use std::path::PathBuf;

use clauseo_protocol::{
    CommandEnvelope, CommandId, CommandOutcome, Cursor, Envelope, FileRefId, FolderRefId,
    HostQuery, MatterId, Principal, QueryResult, Seq, StreamId, ThreadId, ViewEpoch,
};
use clauseo_store::{Committed, Receipt, Reopen, SnapshotIn};
use futures::channel::oneshot;

use super::turn::{TurnJob, TurnTimer};
use crate::agent::AgentInput;
use crate::error::Error;
use crate::handle::{EventSubscription, Subscription};
use crate::matter_worker::MatterReply;
use crate::pipeline::Ticket;

/// A reply to one handle call.
pub(crate) type Reply<T> = oneshot::Sender<Result<T, Error>>;

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
    /// A turn's job.
    #[expect(dead_code, reason = "the turn path's matter jobs use it")]
    Turn(TurnJob),
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
    /// A matter worker's reply.
    Matter {
        matter: MatterId,
        tag: MatterTag,
        reply: MatterReply,
    },
    /// A snapshot compressed on the snapshot thread, to store.
    Snapshot(SnapshotIn),
    /// An agent's event or its end, for the thread it serves.
    #[expect(dead_code, reason = "the turn path posts it")]
    Agent { thread: ThreadId, input: AgentInput },
    /// A turn timer fired.
    Timer(TurnTimer),
    /// A subscription drained its channel after the actor found it full.
    Drained { stream: StreamId, id: SubId },
    /// A subscription was dropped.
    Unsubscribed { stream: StreamId, id: SubId },
    /// End the thread at once (tests); the sender is answered once the store is closed.
    #[cfg_attr(
        not(feature = "test-support"),
        expect(dead_code, reason = "test-support only")
    )]
    Abort(oneshot::Sender<()>),
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Committed { .. } => "Committed",
            Self::Reopened { .. } => "Reopened",
            Self::Receipt { .. } => "Receipt",
            Self::EventPage { .. } => "EventPage",
            Self::Matter { .. } => "Matter",
            Self::Snapshot(_) => "Snapshot",
            Self::Agent { .. } => "Agent",
            Self::Timer(_) => "Timer",
            Self::Drained { .. } => "Drained",
            Self::Unsubscribed { .. } => "Unsubscribed",
            Self::Abort(_) => "Abort",
        };
        f.write_str(name)
    }
}
