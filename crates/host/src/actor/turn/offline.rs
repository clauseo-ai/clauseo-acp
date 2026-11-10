//! Going offline mid-answer (D-050): the agent's own notes about its connection never reach the
//! transcript, and a turn whose agent lost its connection ends `Failed { agent.offline }`, whose
//! words say so and offer Try again.
//!
//! How it arrives:
//! - Codex: codex-acp forwards each Codex warning as answer text, so a dropped connection reads
//!   `Falling back from WebSockets to HTTPS transport. stream disconnected before completion:
//!   Connection refused (os error 61)`. Codex then retries over HTTPS by itself, sending nothing
//!   (its stream errors only reach its own log), until its retries run out and the prompt fails
//!   with a JSON-RPC error whose `data.message` names the failure. Each connect may wait out the
//!   system's connect timeout, so that can take minutes.
//! - Claude: Claude Code retries on its own too, then sends its API error line as answer text
//!   ("API Error: Connection error.") and the prompt fails with "Internal error: <that line>".
//!
//! So: a live answer chunk that is such a note ([`transport_note`]) is held back and logged. A
//! note that names a connection failure ([`is_offline`]) arms a watch: unless the agent sends
//! anything else within [`OFFLINE_WAIT`] (Codex got through over HTTPS), the turn is cancelled
//! through the Cancel path with `agent.offline` as its failure, so the answer so far is kept.
//! A prompt that fails with a connection-class error, or while the watch stands, ends with
//! `agent.offline` too, and skips the Codex limits read, which needs the network as well.
//! Server errors (a status, a rate or usage limit, sign-in) are never offline.

use std::time::Duration;

use clauseo_acp::{AgentUpdate, MessageRole};
use clauseo_protocol::{ContentBlock, ErrorCode, ThreadId, TurnId};

use super::super::Actor;
use super::{TimerKind, TurnTimer, cancel, current};

/// How long a turn waits after the agent said its connection failed, for any sign it got
/// through, before it ends offline.
pub(super) const OFFLINE_WAIT: Duration = Duration::from_secs(30);

/// The start of Codex's warning that its WebSocket stream failed, lowercased.
const CODEX_FALLBACK: &str = "falling back from websockets to https transport";

/// The starts of Claude Code's lines about a failed API call, lowercased.
const CLAUDE_API_ERROR: [&str; 2] = ["api error:", "unable to connect to api"];

/// Words that name a connection that never got through, lowercased: the systems' own error
/// texts (macOS, Linux, Windows), the HTTP stacks' (hyper, reqwest, Node) and the SDKs'.
const CONNECTION: [&str; 26] = [
    "connection refused",
    "actively refused",
    "network is unreachable",
    "unreachable network",
    "unreachable host",
    "no route to host",
    "network is down",
    "failed to lookup address",
    "name or service not known",
    "nodename nor servname",
    "temporary failure in name resolution",
    "no such host is known",
    "could not resolve host",
    "dns error",
    "tcp connect error",
    "operation timed out",
    "connection timed out",
    "did not properly respond after a period of time",
    "connection error.",
    "unable to connect to api",
    "econnrefused",
    "enotfound",
    "eai_again",
    "enetunreach",
    "ehostunreach",
    "etimedout",
];

/// Words that say a server answered, lowercased: never offline, whatever else the text says.
const SERVER: [&str; 15] = [
    "unexpected status",
    "status code",
    "http status",
    "rate limit",
    "usage limit",
    "quota",
    "overloaded",
    "unauthorized",
    "forbidden",
    "high demand",
    "internal server error",
    "bad gateway",
    "service unavailable",
    "gateway timeout",
    "too many requests",
];

/// HTTP statuses that, standing as a word of their own, say a server answered.
const STATUSES: [&str; 8] = ["401", "403", "429", "500", "502", "503", "504", "529"];

/// Whether `text`, an agent's error or note, says it could not reach its service: a refused
/// connection, no network or route, no name resolution, a connect that timed out. A text that
/// says a server answered (a status, a limit, sign-in) is not offline.
pub(super) fn is_offline(text: &str) -> bool {
    let text = text.to_lowercase();
    let server = SERVER.iter().any(|words| text.contains(words))
        || text
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|word| STATUSES.contains(&word));
    !server && CONNECTION.iter().any(|words| text.contains(words))
}

/// What a chunk of answer text is, when it is the agent's note about its connection rather
/// than its answer: `Some(true)` when the note names a connection failure, `Some(false)` for
/// another transport note (Codex's fallback for a reason that is not one), `None` for answer
/// text. Only a whole chunk that starts as the note does counts, so an answer that quotes one
/// stays as written.
pub(super) fn transport_note(text: &str) -> Option<bool> {
    let start = text.trim_start().to_lowercase();
    if start.starts_with(CODEX_FALLBACK) {
        return Some(is_offline(text));
    }
    let claude = CLAUDE_API_ERROR
        .iter()
        .any(|words| start.starts_with(words));
    (claude && is_offline(text)).then_some(true)
}

/// The watch a connection failure arms on the live turn.
#[derive(Debug)]
pub(super) struct Watch {
    /// When it was armed, on the host's monotonic clock.
    since: Duration,
    /// The agent's note, for the turn's failure detail (the log, never shown).
    note: String,
}

/// A live update of `turn`: `true` when it is a transport note, which is logged and held back
/// from the transcript (a connection failure arms the watch); its text, thoughts, tool calls
/// or plan clear the watch, since the agent is at work again (usage and the like do not).
pub(super) fn held_back(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    update: &AgentUpdate,
) -> bool {
    let note = if let AgentUpdate::MessageChunk {
        role: MessageRole::Answer,
        content: ContentBlock::Text { text },
        ..
    } = update
    {
        transport_note(text).map(|offline| (offline, text))
    } else {
        None
    };
    let now = actor.now().mono;
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return note.is_some();
    };
    let Some((offline, text)) = note else {
        let work = matches!(
            update,
            AgentUpdate::MessageChunk { .. }
                | AgentUpdate::ToolCall(_)
                | AgentUpdate::ToolCallUpdate { .. }
                | AgentUpdate::Plan(_)
        );
        if work && current.offline.take().is_some() {
            tracing::info!(%thread, "the agent is at work again after a connection failure");
        }
        return false;
    };
    tracing::warn!(%thread, offline, note = %text, "the agent's note about its connection");
    if offline {
        current.offline = Some(Watch {
            since: now,
            note: text.clone(),
        });
        let timer = TurnTimer {
            thread,
            kind: TimerKind::Offline(turn),
        };
        let _handle = actor.arm_timer(OFFLINE_WAIT, timer);
    }
    true
}

/// The watch's wait ran out: unless the agent sent anything since (or a later note armed it
/// again), the turn is cancelled through the Cancel path and ends `Failed { agent.offline }`.
pub(super) fn on_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread)
        .filter(|current| current.turn == turn && current.live() && current.failure.is_none())
    else {
        return;
    };
    let due = current
        .offline
        .as_ref()
        .is_some_and(|watch| now.saturating_sub(watch.since) >= OFFLINE_WAIT);
    if !due {
        return;
    }
    let note = current.offline.take().map(|watch| watch.note);
    tracing::warn!(%thread, "the agent lost its connection and went quiet; ending the turn");
    current.failure = Some((ErrorCode::AgentOffline, note));
    cancel::cancel_running(actor, thread, None);
}

/// The code a failed prompt of `thread` ends with when it is offline: its error says so, or it
/// failed while the watch stood.
pub(super) fn prompt_failed_offline(
    actor: &mut Actor,
    thread: ThreadId,
    error: &clauseo_acp::Error,
) -> Option<ErrorCode> {
    let watched = current(actor, thread).is_some_and(|current| current.offline.is_some());
    let rpc = matches!(error, clauseo_acp::Error::Rpc { .. });
    let offline = is_offline(&error.to_string()) || error.rpc_data().is_some_and(is_offline);
    (rpc && (watched || offline)).then_some(ErrorCode::AgentOffline)
}

#[cfg(test)]
mod tests;
