//! codex-acp's helper sessions, which `ConnectOptions::helper_sessions` asks for (the AIR
//! capability `nativeSubagentSessions`): the two `subagent_*` updates, read from the raw JSON
//! (the SDK types them only under `unstable_subagents`, which this crate never enables), and the
//! events of a helper's own update turned into helper events. No I/O and no state: which
//! session is a helper's is `state/helper_sessions.rs`.

use clauseo_protocol::{AgentSessionId, CompactString};
use serde_json::Value;

use crate::event::{AcpEvent, AgentUpdate, HelperState, MessageRole, ProtocolWarning};

/// `subagent_spawned`: codex-acp launched a helper (`dist/index.js:37926-37945`).
pub(crate) const SPAWNED: &str = "subagent_spawned";

/// `subagent_state_update`: a helper ended.
pub(crate) const STATE_UPDATE: &str = "subagent_state_update";

/// The words codex-acp puts before the helper's name when it has no task text
/// (`dist/index.js:37932`).
const FILLER: &str = "Delegated task for ";

/// `name` as one of the two `subagent_*` update names, or `None`.
pub(crate) fn subagent_name(name: &str) -> Option<&'static str> {
    [SPAWNED, STATE_UPDATE]
        .into_iter()
        .find(|known| *known == name)
}

/// What a `subagent_*` update says.
#[derive(Debug, PartialEq)]
pub(crate) enum Subagent {
    /// `subagent_spawned`.
    Spawned {
        /// `subagentSessionId`.
        helper: AgentSessionId,
        /// `name`.
        name: CompactString,
        /// `task`, unless empty or the filler for `name`.
        task: Option<CompactString>,
    },
    /// `subagent_state_update`.
    Ended {
        /// `subagentSessionId`.
        helper: AgentSessionId,
        /// `state`, or the error a warning about it carries.
        state: Result<HelperState, String>,
    },
}

/// Reads the raw `update` of a `subagent_*` update named `name` (one [`subagent_name`] gave).
/// An error, for an update without a usable `subagentSessionId` or a `subagent_spawned` without
/// a string `name`, is what an `InvalidUpdate` warning says.
pub(crate) fn subagent(name: &str, update: &Value) -> Result<Subagent, String> {
    let text = |key: &str| update.get(key).and_then(Value::as_str);
    let helper = text("subagentSessionId").ok_or("no subagentSessionId string")?;
    let helper =
        AgentSessionId::new(helper).map_err(|error| format!("subagentSessionId: {error}"))?;
    if name == STATE_UPDATE {
        let state = match text("state") {
            Some("completed") => Ok(HelperState::Completed),
            Some("failed") => Ok(HelperState::Failed),
            Some("cancelled") => Ok(HelperState::Cancelled),
            Some("disconnected") => Ok(HelperState::Disconnected),
            Some(other) => Err(format!("unknown state {other:?}")),
            None => Err("no state string".to_owned()),
        };
        return Ok(Subagent::Ended { helper, state });
    }
    let name = text("name").ok_or("no name string")?;
    let task = text("task")
        .filter(|task| !task.is_empty() && task.strip_prefix(FILLER) != Some(name))
        .map(CompactString::from);
    Ok(Subagent::Spawned {
        helper,
        name: CompactString::from(name),
        task,
    })
}

/// Whether the raw `update` is an `agent_message_chunk` with `_meta.jetbrains.air.phase`
/// `final_answer`: codex-acp marks a helper's report so, and its other text `commentary`.
pub(crate) fn final_answer(update: &Value) -> bool {
    let phase = update
        .get("_meta")
        .and_then(|meta| meta.get("jetbrains"))
        .and_then(|meta| meta.get("air"))
        .and_then(|air| air.get("phase"))
        .and_then(Value::as_str);
    phase == Some("final_answer")
        && update.get("sessionUpdate").and_then(Value::as_str) == Some("agent_message_chunk")
}

/// The warning that stands for a plan limit, a too-long turn or a lost connection a helper's
/// update reports: its raw AIR `sessionFailure`'s `category`, `severity` and `title`, each
/// `None` when missing or not a string (as for Claude's `_claude/rateLimit`, which has none).
pub(crate) fn failure_warning(update: &Value) -> ProtocolWarning {
    let failure = update
        .get("_meta")
        .and_then(|meta| meta.get("jetbrains"))
        .and_then(|meta| meta.get("air"))
        .and_then(|air| air.get("sessionFailure"));
    let text = |key: &str| {
        failure
            .and_then(|failure| failure.get(key))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    ProtocolWarning::SessionFailure {
        category: text("category"),
        severity: text("severity"),
        title: text("title"),
    }
}

/// Turns the events helper session `helper`'s update mapped to (on the root session) into the
/// helper's: each `Update` becomes `HelperUpdate`, or with `final_answer` an answer chunk
/// becomes `HelperAnswer`; a warning names `helper`, the session the frame named; a plan
/// limit, too-long turn or lost connection becomes the warning `failure`, so a helper's own
/// failure never stops or marks the root's turn.
pub(crate) fn helper_events(
    helper: &AgentSessionId,
    final_answer: bool,
    failure: &ProtocolWarning,
    events: &mut Vec<AcpEvent>,
) {
    let id = CompactString::from(helper.as_str());
    let warning = |warning| AcpEvent::Warning {
        session: Some(helper.clone()),
        warning,
    };
    let all = std::mem::take(events);
    *events = all
        .into_iter()
        .map(|event| match event {
            AcpEvent::Update {
                session,
                origin,
                update:
                    AgentUpdate::MessageChunk {
                        role: MessageRole::Answer,
                        content,
                        ..
                    },
            } if final_answer => AcpEvent::HelperAnswer {
                session,
                origin,
                helper: id.clone(),
                content,
            },
            AcpEvent::Update {
                session,
                origin,
                update,
            } => AcpEvent::HelperUpdate {
                session,
                origin,
                helper: id.clone(),
                update,
            },
            AcpEvent::Warning { warning: inner, .. } => warning(inner),
            AcpEvent::Limit { .. } | AcpEvent::TooLong { .. } | AcpEvent::ConnectionLost { .. } => {
                warning(failure.clone())
            }
            other @ (AcpEvent::HelperUpdate { .. }
            | AcpEvent::HelperInBackground { .. }
            | AcpEvent::HelperSpawned { .. }
            | AcpEvent::HelperAnswer { .. }
            | AcpEvent::HelperEnded { .. }
            | AcpEvent::PermissionRequested { .. }
            | AcpEvent::ElicitationRequested { .. }
            | AcpEvent::ElicitationCompleted { .. }
            | AcpEvent::UpdateSkipped { .. }) => other,
        })
        .collect();
}
