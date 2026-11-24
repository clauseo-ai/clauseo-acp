//! Claude Code's task frames, which claude-agent-acp 0.110.1 passes on as the notification
//! `_claude/sdkMessage` (`{sessionId, message}`) when the session's
//! `_meta.claudeCode.emitRawSDKMessages` names them (`dist/v1/acp-agent.js:2948-2953`, the
//! filter at `:8387-8395`). A helper (and a shell command) runs as a task:
//!
//! - `task_started` names its `task_id` and the call that launched it (`tool_use_id`).
//! - `task_updated` names only the `task_id`, with `patch.status` (`pending`, `running`,
//!   `paused`, or the settles `completed`, `failed`, `killed`). The adapter's note: one comes on
//!   every change of status.
//! - `task_notification` names both ids, with `status` (`completed`, `failed`, `stopped`); it
//!   may be skipped.
//!
//! Only the ids, the frame's `type` and `subtype`, and the status are read. The frames also
//! carry the helper's task text, an output file's path, a summary, the handed-back report and
//! usage numbers: none of it is read, kept or logged here.

use clauseo_protocol::{CompactString, HelperOutcome};
use serde_json::Value;

/// The method claude-agent-acp sends an engine frame as.
pub(crate) const METHOD: &str = "_claude/sdkMessage";

/// What one task frame says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Frame {
    /// Task `task` runs for the call `launch` (`task_started`).
    Started {
        task: CompactString,
        launch: CompactString,
    },
    /// The task of the call `launch` settled with `outcome` (`task_notification`); `task`, when
    /// the frame names it.
    Notified {
        task: Option<CompactString>,
        launch: CompactString,
        outcome: HelperOutcome,
    },
    /// Task `task` settled with `outcome` (`task_updated` with a settle status).
    Updated {
        task: CompactString,
        outcome: HelperOutcome,
    },
}

/// The task frame `message` holds, or `None`: another frame (`init`), a `task_updated` whose
/// status is not a settle (or that it does not know), or a frame missing an id or a status.
pub(crate) fn read(message: &Value) -> Option<Frame> {
    if message.get("type").and_then(Value::as_str) != Some("system") {
        return None;
    }
    let id = |key| {
        message
            .get(key)
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .map(CompactString::from)
    };
    match message.get("subtype").and_then(Value::as_str)? {
        "task_started" => Some(Frame::Started {
            task: id("task_id")?,
            launch: id("tool_use_id")?,
        }),
        "task_notification" => {
            let status = message.get("status").and_then(Value::as_str)?;
            Some(Frame::Notified {
                task: id("task_id"),
                launch: id("tool_use_id")?,
                outcome: notified(status),
            })
        }
        "task_updated" => {
            let status = message.get("patch")?.get("status")?.as_str()?;
            Some(Frame::Updated {
                task: id("task_id")?,
                outcome: updated(status)?,
            })
        }
        _ => None,
    }
}

/// A `task_notification`'s status: every one is a settle, so one it does not know ends the
/// helper `Failed`.
fn notified(status: &str) -> HelperOutcome {
    match status {
        "completed" => HelperOutcome::Done,
        "stopped" | "killed" => HelperOutcome::Stopped,
        _ => HelperOutcome::Failed,
    }
}

/// A `task_updated`'s status, when it is a settle. One it does not know is not taken as a
/// settle, since a `task_updated` also says a task waits or runs again.
fn updated(status: &str) -> Option<HelperOutcome> {
    match status {
        "completed" => Some(HelperOutcome::Done),
        "failed" => Some(HelperOutcome::Failed),
        "killed" | "stopped" => Some(HelperOutcome::Stopped),
        _ => None,
    }
}
