//! What the turn path's own tasks post back to the actor: agent call results and whole-log
//! reads.

use clauseo_acp::{AgentInfo, PromptEnd, SessionStart};
use clauseo_protocol::{AgentChoiceId, ConfigOption, Envelope, ThreadId, TurnId};

use super::super::commands::CommandCtx;
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
        result: Result<super::launch_checks::Prepared, crate::error::Error>,
    },
    /// A whole-log read of `thread`'s stream for `why`.
    Log {
        thread: ThreadId,
        why: LogFor,
        result: Result<Vec<Envelope>, clauseo_store::Error>,
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
    /// A Codex session's checks before its attach (the ask mode, the guard's trust) ended.
    Prepared {
        loaded: bool,
        result: Result<SessionStart, crate::error::Error>,
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
    Config {
        ctx: CommandCtx,
        result: Result<Vec<ConfigOption>, clauseo_acp::Error>,
    },
    Mode {
        ctx: CommandCtx,
        mode: AgentChoiceId,
        result: Result<(), clauseo_acp::Error>,
    },
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
}
