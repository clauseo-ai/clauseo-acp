//! The actor's state types besides `Actor` itself: what a commit continues with, the matters
//! and threads the host knows, the work start-up and shutdown wait for, and the host's phase.

use std::sync::mpsc;

use clauseo_protocol::{CommandId, CommandOutcome, MatterId};
use clauseo_store::{MatterRow, ThreadRow};

use super::commands::AfterCommand;
use super::input::{self, MatterTag};
use super::turn;
use crate::error::Error;
use crate::matter_worker::MatterWorker;

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
    /// The retry of the commit logging a restore of this matter.
    Restore(MatterId),
    /// The commit logging snoozes' wakes (`triage/wake.rs`).
    Wake,
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
    /// How and when its last turn ended, for the matter's status: from the loaded state's last
    /// step when the stream is unloaded, and from each durable `TurnEnded`.
    pub(crate) last_stop: Option<super::rail::Stop>,
    pub(crate) turn: turn::TurnState,
    /// The papers its undos removed that the host still looks for (`turn/came_back.rs`).
    pub(crate) came_back: turn::CameBackWatches,
}

/// Work in flight that start-up and shutdown wait for.
#[derive(Debug, Default)]
pub(crate) struct Work {
    pub(crate) commits: usize,
    pub(crate) loads: usize,
    pub(crate) receipts: usize,
    pub(crate) matter_jobs: usize,
    pub(crate) snapshots: usize,
    /// Run folders being removed off the actor (`launch_checks::discard`), with their browser
    /// claims: shutdown answers only once they are gone.
    pub(crate) run_removals: usize,
}

impl Work {
    pub(super) fn idle(&self) -> bool {
        self.commits == 0
            && self.loads == 0
            && self.receipts == 0
            && self.matter_jobs == 0
            && self.snapshots == 0
            && self.run_removals == 0
    }
}

/// Where the host is in its life.
#[derive(Debug)]
pub(crate) enum HostPhase {
    /// Start-up work runs; `ready` hears once it is done.
    Starting(mpsc::Sender<Result<(), Error>>),
    Serving,
    /// Shutdown runs; every reply hears once the store is closed.
    Stopping(Vec<input::Reply<()>>),
}
