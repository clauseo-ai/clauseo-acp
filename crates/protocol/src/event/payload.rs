//! Every event payload struct, each the payload of the `DomainEvent` variant of the same name.
//! One file per subject: `thread.rs` (the thread and its agent session), `turn.rs` (a turn,
//! from the lawyer's message to its end), `helper.rs` (a helper's start, words and end),
//! `tool.rs` (tool calls, web searches and pages, permissions and elicitations) and `matter.rs`
//! (the matter-level events). The value types
//! they hold live by topic outside `event`: `content.rs`, `tool.rs`, `session.rs`, `turn.rs`,
//! `approval.rs`, `matter.rs` and `limits.rs`.

mod helper;
mod matter;
mod thread;
mod tool;
mod turn;

pub use helper::{HelperEnded, HelperStarted, HelperText};
pub use matter::{
    CheckpointRecorded, MatterCreated, MatterRelocated, OutsideEdits, VersionRestored,
};
pub use thread::{
    AgentNamed, AgentRetryEnded, AgentRetrying, AgentSessionAttached, AgentSessionDetached,
    AgentSwitched, CommandsAvailable, CompactionChanged, ConfigOptionsReplaced, ContextHandoff,
    Endpoint, HandoffSettled, HandoffStatus, ModeChanged, Notice, PlanReplaced, ThreadArchived,
    ThreadCreated, ThreadDone, ThreadRenamed, ThreadSnoozed, UsageUpdated,
};
pub use tool::{
    ElicitationExpired, ElicitationRequested, ElicitationResolved, PageVisited, PermissionExpired,
    PermissionRequested, PermissionResolved, ToolCallStarted, ToolCallUpdated, WebResult,
    WebSearchRecorded,
};
pub use turn::{
    ContentAppended, FilesChanged, MatterHold, QueuedPromptEdited, SteerResolved, StepUndone,
    TextAppended, TurnEnded, TurnFailed, TurnQueued, TurnStarted, TurnStoppedAtLimit, UserMessage,
};
