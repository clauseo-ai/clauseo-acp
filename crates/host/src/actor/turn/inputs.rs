//! What the turn path's own tasks post back to the actor: agent call results and whole-log
//! reads.

use clauseo_acp::{AgentInfo, PromptEnd, SessionStart};
use clauseo_protocol::{
    AgentChoiceId, ConfigOption, ConfigValue, Envelope, QueryResult, ThreadId, TurnId,
};

use super::super::commands::CommandCtx;
use super::super::input::Reply;
use super::undo;

/// A result the turn path's own tasks post back: agent calls and store reads.
#[derive(Debug)]
pub(crate) enum TurnInput {
    /// A call to the agent `agent` of `thread` returned.
    Agent {
        thread: ThreadId,
        agent: u64,
        reply: AgentReply,
    },
    /// A launch's folder checks `id` (`launch_checks.rs`) answered.
    Prepared {
        thread: ThreadId,
        id: u64,
        result: Result<Box<super::launch_checks::Prepared>, crate::error::Error>,
    },
    /// A whole-log read of `thread`'s stream for `why`.
    Log {
        thread: ThreadId,
        why: LogFor,
        result: Result<Vec<Envelope>, clauseo_store::Error>,
    },
    /// A run folder's removal off the actor (`launch_checks::discard`) ended.
    RunRemoved {
        /// The session.
        key: crate::config::SessionKey,
        /// Whether the run was freed (its slot goes back), or held.
        freed: bool,
    },
    /// A check for papers an undo removed that came back is due (`came_back.rs`).
    CameBack(super::came_back::Check),
    /// The host's probe of the agent's service for `thread`'s `turn` answered (`offline.rs`).
    OfflineProbe {
        thread: ThreadId,
        turn: TurnId,
        reached: bool,
    },
}

/// An agent call's result.
#[derive(Debug)]
pub(crate) enum AgentReply {
    Initialized(Result<AgentInfo, clauseo_acp::Error>),
    /// `session/load`, `session/resume` or `session/new` returned; `loaded` for the first
    /// two, which continue the stored session.
    Session {
        loaded: bool,
        result: Result<SessionStart, clauseo_acp::Error>,
    },
    /// A session's work before its attach (Codex's ask mode and guard's trust, Claude's mode,
    /// the picks) ended; `picked` says what the picks did.
    Prepared {
        loaded: bool,
        result: Result<SessionStart, crate::error::Error>,
        picked: super::picks::Picked,
    },
    Prompt {
        turn: TurnId,
        result: Result<PromptEnd, clauseo_acp::Error>,
    },
    /// A Codex prompt's `.codex` re-check (or its wait) refused it before it was sent.
    PromptRefused {
        turn: TurnId,
        error: crate::error::Error,
    },
    /// A Codex session call's `.codex` re-check refused it before it was sent.
    Refused(crate::error::Error),
    /// A `SetConfigOption` of `pick` answered.
    Config {
        ctx: CommandCtx,
        pick: (AgentChoiceId, ConfigValue),
        result: Result<Vec<ConfigOption>, clauseo_acp::Error>,
    },
    Mode {
        ctx: CommandCtx,
        mode: AgentChoiceId,
        result: Result<(), clauseo_acp::Error>,
    },
    /// The switch back after a Claude agent moved itself to a mode Clauseo doesn't run it in
    /// (`forced_mode.rs`) answered: the mode it was switched to, and the answer.
    ModeReset(&'static str, Result<(), clauseo_acp::Error>),
    /// A steer into the running turn answered (`queue/steer.rs`).
    Steer(Box<super::queue::Steered>),
}

/// Why a thread's whole log was read.
#[derive(Debug)]
pub(crate) enum LogFor {
    /// Replay dedupe before `session/load`.
    Replay,
    Reconcile,
    Undo(Box<undo::UndoAsk>),
    /// A `PreviewUndo` query of `turn`'s step, answered on `reply`.
    PreviewUndo {
        turn: TurnId,
        reply: Reply<QueryResult>,
    },
}
