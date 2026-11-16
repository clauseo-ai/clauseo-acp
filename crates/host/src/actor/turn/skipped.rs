//! The tool calls whose `session/update` the ACP reader dropped over its line cap with no status
//! it could pass on (`AcpEvent::UpdateSkipped`). The dropped update may have been the call's
//! last, so a turn the agent ends well completes those calls that are still unfinished, status
//! only ([`TurnClose::complete_tools`](super::close::TurnClose)). Every other unfinished call is
//! left as it is: an agent may finish a tool call in a later turn (codex-acp sends a later update
//! for a call its turn left open), and that update then gives its real status.
//!
//! When the dropped line showed no tool call id (or no session), or more than [`MAX_TOOLS`]
//! were dropped in one turn, the turn's close completes every tool call of the turn still
//! unfinished, as before tool calls were told apart.

use clauseo_acp::Origin;
use clauseo_protocol::{AgentSessionId, CompactString, StreamId, ThreadId, ToolCallId, TurnId};

use super::super::Actor;
use super::close::{TurnClose, completes_tools};
use super::{current, own_turn};

/// The most tool calls named one by one in a turn; past it the turn is [`Skipped::Unknown`].
const MAX_TOOLS: usize = 64;

/// The tool calls of a turn whose update was dropped with no status read.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) enum Skipped {
    /// None.
    #[default]
    None,
    /// These, by the agent's session and `toolCallId`.
    Tools(Vec<(AgentSessionId, CompactString)>),
    /// A dropped update whose tool call is not known: every unfinished tool call of the turn.
    Unknown,
}

impl Skipped {
    fn add(&mut self, tool: Option<(AgentSessionId, CompactString)>) {
        let Some(tool) = tool else {
            *self = Self::Unknown;
            return;
        };
        match self {
            Self::None => *self = Self::Tools(vec![tool]),
            Self::Tools(tools) if tools.contains(&tool) => {}
            Self::Tools(tools) if tools.len() < MAX_TOOLS => tools.push(tool),
            Self::Tools(_) => *self = Self::Unknown,
            Self::Unknown => {}
        }
    }
}

/// `AcpEvent::UpdateSkipped` from `thread`'s agent: kept on the live turn it belongs to. An
/// update of a prompt's turn names it; an `Idle` one belongs to the live turn, if any (an
/// agent-started turn, or one whose session the dropped line did not show). A late or replayed
/// one is dropped.
pub(super) fn on_skipped(
    actor: &mut Actor,
    thread: ThreadId,
    session: Option<AgentSessionId>,
    origin: Origin,
    acp_tool_call_id: Option<CompactString>,
) {
    let turn = match origin {
        Origin::Turn(turn) => Some(turn),
        Origin::Idle => own_turn::live(actor, thread).or_else(|| {
            let current = current(actor, thread).filter(|current| current.live())?;
            Some(current.turn)
        }),
        Origin::Late(_) | Origin::Replay => None,
    };
    let tool = session.zip(acp_tool_call_id);
    tracing::debug!(%thread, known = tool.is_some(), "a tool call's update was dropped");
    if let Some(current) = current(actor, thread).filter(|current| Some(current.turn) == turn) {
        current.skipped.add(tool);
    }
}

/// What the close of `thread`'s current turn completes: the tool calls the turn kept, when it
/// stops well with no failure ([`completes_tools`]).
pub(super) fn to_complete(actor: &mut Actor, thread: ThreadId, close: &TurnClose) -> Skipped {
    if close.failure.is_some() || !completes_tools(close.stop) {
        return Skipped::None;
    }
    current(actor, thread)
        .filter(|current| current.turn == close.turn)
        .map(|current| std::mem::take(&mut current.skipped))
        .unwrap_or_default()
}

/// The tool calls `skipped` names that are still unfinished, as sent; for
/// [`Skipped::Unknown`], every unfinished tool call of `turn`. A named call started in an
/// earlier turn counts too: the update dropped in this one was its last.
pub(super) fn unfinished(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    skipped: &Skipped,
) -> Vec<ToolCallId> {
    let Some(loaded) = actor.loaded(StreamId::Thread(thread)) else {
        return Vec::new();
    };
    match skipped {
        Skipped::None => Vec::new(),
        Skipped::Tools(tools) => tools
            .iter()
            .filter_map(|(session, acp_id)| loaded.sent.tool_call(session, acp_id))
            .filter(|(_, tool)| !tool.finished)
            .map(|(id, _)| id)
            .collect(),
        Skipped::Unknown => loaded.sent.unfinished_tools(turn).collect(),
    }
}
