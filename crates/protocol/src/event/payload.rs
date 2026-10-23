//! Every event payload struct, each the payload of the `DomainEvent` variant of the same name.
//! One file per subject: `thread.rs` (the thread and its agent session), `turn.rs` (a turn,
//! from the lawyer's message to its end), `tool.rs` (tool calls, permissions and elicitations)
//! and `matter.rs` (the matter-level events). The value types they hold live by topic outside
//! `event`: `content.rs`, `tool.rs`, `session.rs`, `turn.rs`, `approval.rs`, `matter.rs` and
//! `limits.rs`.

mod matter;
mod thread;
mod tool;
mod turn;

pub use matter::{CheckpointRecorded, MatterCreated, OutsideEdits, VersionRestored};
pub use thread::{
    AgentSessionAttached, AgentSessionDetached, CommandsAvailable, CompactionChanged,
    ConfigOptionsReplaced, ModeChanged, Notice, PlanReplaced, ThreadArchived, ThreadCreated,
    ThreadRenamed, UsageUpdated,
};
pub use tool::{
    ElicitationExpired, ElicitationRequested, ElicitationResolved, PermissionExpired,
    PermissionRequested, PermissionResolved, ToolCallStarted, ToolCallUpdated,
};
pub use turn::{
    ContentAppended, FilesChanged, QueuedPromptEdited, SteerResolved, StepUndone, TextAppended,
    TurnEnded, TurnFailed, TurnQueued, TurnStarted, TurnStoppedAtLimit, UserMessage,
};
