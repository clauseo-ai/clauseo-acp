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
//!   ("API Error: Unable to connect to API (ECONNREFUSED)") and the prompt fails with
//!   "Internal error: <that line>".
//! - The window: `SetNetwork { online: false }` when the webview's `offline` event fires.
//!
//! So a watch arms on the live turn: on an agent note that names a connection failure
//! ([`classify::transport_note`]), or for every running turn while the window says offline.
//! Codex's note is surely its own and is dropped; Claude's line is only suspected (a model could
//! write the same words), so it is held in the watch and put back in order if the agent goes on
//! or the turn ends any other way, and dropped only when the turn ends offline. Any agent event of the
//! turn but usage, commands, config options, mode and title clears the watch: the agent got
//! through. Unless it is cleared within [`OFFLINE_WAIT`], and no permission or elicitation waits
//! on the lawyer, the turn is cancelled through the Cancel path with `agent.offline` as its
//! failure, so the answer so far is kept. A prompt that fails with a connection-class error, or
//! while the watch stands, ends `agent.offline` too, unless its error says a server answered or
//! asks for a sign-in. The log gets fixed fields only, never the agent's text (decision 0047).

use std::time::Duration;

use clauseo_acp::{AgentUpdate, MessageRole};
use clauseo_protocol::{AgentSessionId, CommandOutcome, ContentBlock, ErrorCode, ThreadId, TurnId};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::{TimerKind, TurnTimer, cancel, current, sign_in, updates};

pub(super) mod classify;

/// How long a turn waits after the agent said its connection failed, or the window said the
/// computer went offline, for any sign the agent got through, before it ends offline.
pub(super) const OFFLINE_WAIT: Duration = Duration::from_secs(30);

/// The failure's detail when the window's report armed the watch.
const NETWORK_DETAIL: &str = "the window said the computer is offline and the agent sent \
                              nothing for 30 s";

/// The watch on the live turn.
#[derive(Debug)]
pub(super) struct Watch {
    /// When it was armed, on the host's monotonic clock.
    since: Duration,
    /// The agent's note that armed it, for the failure's detail (the event log, never shown);
    /// `None` when the window's report armed it.
    note: Option<String>,
    /// Claude's suspected lines, held back in order, with their sessions.
    held: Vec<(AgentSessionId, AgentUpdate)>,
}

/// A live update of `turn`: `None` when it is held back or dropped as the agent's note about
/// its connection (a connection failure arms the watch), else the update to log. Any other
/// agent work clears the watch and puts the held lines back first.
pub(super) fn on_live(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    session: &AgentSessionId,
    update: AgentUpdate,
) -> Option<AgentUpdate> {
    let note = if let AgentUpdate::MessageChunk {
        role: MessageRole::Answer,
        content: ContentBlock::Text { text },
        ..
    } = &update
    {
        classify::transport_note(text).map(|note| (note, text.clone()))
    } else {
        None
    };
    let Some((note, text)) = note else {
        let idle = matches!(
            update,
            AgentUpdate::Usage { .. }
                | AgentUpdate::Commands(_)
                | AgentUpdate::ConfigOptions(_)
                | AgentUpdate::CurrentMode { .. }
                | AgentUpdate::SessionTitle { .. }
        );
        if !idle {
            agent_active(actor, thread, turn);
        }
        return Some(update);
    };
    let (source, offline) = (note.source, note.offline);
    tracing::warn!(%thread, source, offline, len = text.len(), "the agent's note about its connection");
    if offline {
        let now = actor.now().mono;
        let current = current(actor, thread).filter(|current| current.turn == turn)?;
        let watch = current.offline.get_or_insert_with(|| Watch {
            since: now,
            note: None,
            held: Vec::new(),
        });
        watch.since = now;
        watch.note = Some(text);
        if !note.sure {
            watch.held.push((session.clone(), update));
        }
        arm_timer(actor, thread, turn);
    }
    None
}

/// The agent of `turn` is at work (an answer, a tool call, a permission request, a limit): the
/// watch goes, and the lines it held go back into the answer, in order.
pub(super) fn agent_active(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let watch = current(actor, thread)
        .filter(|current| current.turn == turn && current.live())
        .and_then(|current| current.offline.take());
    if let Some(watch) = watch {
        tracing::info!(%thread, held = watch.held.len(), "the agent is at work again");
        put_back(actor, thread, turn, watch.held);
    }
}

/// `thread`'s turn ends with `failure` (else the one it holds), or its agent ended
/// (`agents::on_ended`, while the agent is still there to log them): unless that is
/// `agent.offline`, the held lines go back into the answer.
pub(super) fn turn_ending(actor: &mut Actor, thread: ThreadId, failure: Option<ErrorCode>) {
    let Some(current) = current(actor, thread) else {
        return;
    };
    let failure = failure.or_else(|| current.failure.as_ref().map(|(code, _)| *code));
    if failure == Some(ErrorCode::AgentOffline) {
        return;
    }
    let turn = current.turn;
    if let Some(watch) = current.offline.take() {
        put_back(actor, thread, turn, watch.held);
    }
}

fn put_back(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    held: Vec<(AgentSessionId, AgentUpdate)>,
) {
    for (session, update) in held {
        updates::log_live(actor, thread, turn, &session, update);
    }
}

fn arm_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let timer = TurnTimer {
        thread,
        kind: TimerKind::Offline(turn),
    };
    let _handle = actor.arm_timer(OFFLINE_WAIT, timer);
}

/// `SetNetwork`: the window says whether the computer is online. Offline arms the watch on
/// every running turn (a turn that starts later gets it from [`turn_started`]); online takes
/// back each watch it armed, never one an agent's note armed. Answers `Done`, no receipt.
pub(crate) fn set_network(actor: &mut Actor, ctx: &CommandCtx, online: bool) {
    if actor.turn_path.offline == online {
        tracing::info!(online, "the window's network signal");
    }
    actor.turn_path.offline = !online;
    let threads: Vec<ThreadId> = actor.threads.keys().copied().collect();
    for thread in threads {
        if online {
            let Some(current) = current(actor, thread) else {
                continue;
            };
            if current
                .offline
                .as_ref()
                .is_some_and(|watch| watch.note.is_none())
            {
                current.offline = None;
            }
        } else {
            network_watch(actor, thread);
        }
    }
    actor.finish_command(ctx.id, Ok(CommandOutcome::Done));
}

/// `thread`'s turn went live (its prompt is out): while the window says offline, it is watched.
pub(super) fn turn_started(actor: &mut Actor, thread: ThreadId) {
    if actor.turn_path.offline {
        network_watch(actor, thread);
    }
}

/// Arms the window's watch on `thread`'s live turn, unless one stands already.
fn network_watch(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread)
        .filter(|current| current.live() && current.failure.is_none() && current.offline.is_none())
    else {
        return;
    };
    current.offline = Some(Watch {
        since: now,
        note: None,
        held: Vec::new(),
    });
    let turn = current.turn;
    arm_timer(actor, thread, turn);
}

/// The watch's wait ran out: unless the agent sent anything since (or a later note armed it
/// again), or a permission or elicitation waits on the lawyer, the turn is cancelled through
/// the Cancel path and ends `Failed { agent.offline }`.
pub(super) fn on_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let now = actor.now().mono;
    let asking = updates::agent(actor, thread).is_some_and(|agent| {
        !(agent.permissions.is_empty()
            && agent.elicitations.is_empty()
            && agent.url_elicitations.is_empty())
    });
    let Some(current) = current(actor, thread)
        .filter(|current| current.turn == turn && current.live() && current.failure.is_none())
    else {
        return;
    };
    let due = current
        .offline
        .as_ref()
        .is_some_and(|watch| now.saturating_sub(watch.since) >= OFFLINE_WAIT);
    if !due || asking {
        return;
    }
    let note = current.offline.take().and_then(|watch| watch.note);
    let by = if note.is_some() {
        "agent_note"
    } else {
        "window"
    };
    tracing::warn!(%thread, by, "the agent went quiet while offline; ending the turn");
    let detail = note.unwrap_or_else(|| NETWORK_DETAIL.to_owned());
    current.failure = Some((ErrorCode::AgentOffline, Some(detail)));
    cancel::cancel_running(actor, thread, None);
}

/// The code a failed prompt of `thread` ends with when it is offline. A sign-in the error asks
/// for comes first, and an error that says a server answered (a status, a limit) is never
/// offline, watch or not; else the error names a connection failure, or the watch stands.
/// The RPC error's `data` (codex-acp's `data.message`) is read beside its text.
pub(super) fn prompt_failed_offline(
    actor: &mut Actor,
    thread: ThreadId,
    error: &clauseo_acp::Error,
) -> Option<ErrorCode> {
    if !matches!(error, clauseo_acp::Error::Rpc { .. }) {
        return None;
    }
    let text = error.to_string();
    let data = error.rpc_data();
    if sign_in::code_of(actor, error.code(), &text, data) == ErrorCode::AgentAuthRequired {
        return None;
    }
    let both = [Some(text.as_str()), data];
    let texts = || both.iter().flatten();
    let server = texts().any(|text| classify::says_server(text));
    let connection = texts().any(|text| classify::says_connection(text));
    let watched = current(actor, thread).is_some_and(|current| current.offline.is_some());
    (!server && (watched || connection)).then_some(ErrorCode::AgentOffline)
}

#[cfg(test)]
mod tests;
