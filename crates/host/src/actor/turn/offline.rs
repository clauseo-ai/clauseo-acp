//! Going offline mid-answer (D-050): the agent's own notes about its connection never reach the
//! transcript, and a turn whose agent lost its connection ends `Failed { agent.offline }`, whose
//! words say so and offer Try again.
//!
//! How it arrives:
//! - Codex (codex-acp 2.2.2, to a client like Clauseo that advertises notices and the AIR
//!   `sessionFailure`): each retry Codex makes by itself is a `session_info_update` whose
//!   `sessionFailure` has category `connection` and severity `warning`, titled with Codex's
//!   `Reconnecting... 2/5` or `Reconnecting... waiting for network` (Codex 0.162 retries a
//!   failed connection without end, at most a minute apart). Its warning that the WebSocket
//!   failed is a `notice`: `Falling back from WebSockets to HTTPS transport. stream disconnected
//!   before completion: Connection refused (os error 61)`. Once its retries run out, the prompt
//!   answers `end_turn` with a `connection` failure of severity `error` in its `_meta`. To a
//!   client without those, codex-acp writes the warning as answer text and fails the prompt with
//!   a JSON-RPC error whose `data.message` names the failure.
//! - Claude: Claude Code retries on its own too, then sends its API error line as answer text
//!   ("API Error: Unable to connect to API (ECONNREFUSED)") and the prompt fails with
//!   "Internal error: <that line>".
//! - The window: `SetNetwork { online: false }` when the webview's `offline` event fires.
//!
//! So a watch arms on the live turn: on a `connection` failure ([`on_connection_lost`]), on an
//! agent note, in its answer text or a notice, that names a connection failure
//! ([`classify::transport_note`]), or for every running turn while the window says offline.
//! Codex's note is surely its own and is dropped; Claude's line is only suspected (a model could
//! write the same words), so it is held in the watch and put back in order if the agent goes on
//! or the turn ends any other way, and dropped only when the turn ends offline. Any agent event
//! of the turn but usage, commands, config options, mode and title clears the watch: the agent
//! got through. Unless it is cleared within [`OFFLINE_WAIT`], the turn is cancelled through the
//! Cancel path with `agent.offline` as its failure, so the answer so far is kept. A turn waiting
//! on the lawyer (a permission, an elicitation) waits again instead; so does one running a tool
//! call under a watch only the window armed, for up to [`WORKING_LIMIT`] (after an agent's note
//! an open tool call is stale). Either watch ends only once the host's own probe (`probe.rs`)
//! cannot reach the agent's service either: Codex's fallback note means it retries over HTTPS,
//! which a blip or a proxy that blocks only WebSocket connections lets through, and a reasoning
//! model may stay silent past the wait. A probe that gets through makes it wait again. A prompt
//! that fails with a connection-class error, or while an agent's note armed the watch, ends
//! `agent.offline` too, unless its error says a server answered or asks for a sign-in; so does
//! one that ends carrying a `connection` failure. The log gets fixed fields only, never the
//! agent's text (decision 0047).

use std::time::Duration;

use clauseo_acp::{AgentUpdate, ConnectionLost, MessageRole, Origin, PromptEnd};
use clauseo_protocol::{
    AgentSessionId, CommandOutcome, ContentBlock, ErrorCode, StreamId, ThreadId, TurnId,
};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::{TimerKind, TurnPhase, TurnTimer, cancel, current, sign_in, updates};

pub(super) mod classify;
pub(crate) mod probe;

/// How long a turn waits after the agent said its connection failed, or the window said the
/// computer went offline, for any sign the agent got through, before it ends offline.
pub(super) const OFFLINE_WAIT: Duration = Duration::from_secs(30);

/// How long a watch only the window armed lets a tool call still running keep the turn going:
/// past it, a probe that cannot reach the agent's service ends the turn offline anyway.
pub(super) const WORKING_LIMIT: Duration = Duration::from_secs(600);

/// The failure's detail when the window's report armed the watch.
const NETWORK_DETAIL: &str = "the window said the computer is offline, the agent sent \
                              nothing for 30 s and the host's probe did not get through";

/// The note a `connection` failure without a title arms the watch with.
const LOST_DETAIL: &str = "the agent said it lost its connection to its service";

/// The watch on the live turn.
#[derive(Debug)]
pub(super) struct Watch {
    /// When it was armed, or armed again, on the host's monotonic clock.
    since: Duration,
    /// When it was first armed; arming it again leaves this.
    first: Duration,
    /// The agent's note that armed it, for the failure's detail (the event log, never shown);
    /// `None` when the window's report armed it.
    note: Option<String>,
    /// Claude's suspected lines, held back in order, with their sessions.
    held: Vec<(AgentSessionId, AgentUpdate)>,
    /// The host's probe runs for it (a watch the window armed, whose wait ran out).
    probing: bool,
}

#[cfg(test)]
impl Watch {
    /// When it was armed, or armed again.
    pub(super) const fn since(&self) -> Duration {
        self.since
    }

    /// Whether the host's probe runs for it.
    pub(super) const fn probing(&self) -> bool {
        self.probing
    }

    /// The agent's note that armed it last.
    pub(super) fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

/// The text of an update that may be the agent's note about its connection: answer text, or a
/// notice (codex-acp sends Codex's warnings as notices to a client that takes them).
fn note_text(update: &AgentUpdate) -> Option<&str> {
    if let AgentUpdate::MessageChunk {
        role: MessageRole::Answer,
        content: ContentBlock::Text { text },
        ..
    }
    | AgentUpdate::Notice { text, .. } = update
    {
        Some(text)
    } else {
        None
    }
}

/// Whether an update outside a turn is Codex's note about its connection, which never reaches
/// the transcript, turn or not.
pub(super) fn outside_note(thread: ThreadId, update: &AgentUpdate) -> bool {
    let sure = note_text(update)
        .and_then(classify::transport_note)
        .is_some_and(|note| note.sure);
    if sure {
        tracing::info!(%thread, "the agent's note about its connection, outside a turn");
    }
    sure
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
    let note = note_text(&update)
        .and_then(|text| classify::transport_note(text).map(|note| (note, text.to_owned())));
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
        let held = (!note.sure).then(|| (session.clone(), update));
        arm_note(actor, thread, turn, text, held);
    }
    None
}

/// An AIR `sessionFailure` of category `connection` (codex-acp's retry warnings, or its
/// giving up): during `thread`'s live turn it arms the watch as Codex's fallback note does, and
/// the host's probe decides once the agent stays quiet. It is never shown.
pub(super) fn on_connection_lost(
    actor: &mut Actor,
    thread: ThreadId,
    origin: Origin,
    lost: &ConnectionLost,
) {
    let turn = match origin {
        Origin::Turn(turn) => Some(turn),
        Origin::Late(_) | Origin::Replay | Origin::Idle => None,
    };
    let live = turn.filter(|turn| {
        current(actor, thread).is_some_and(|current| current.turn == *turn && current.live())
    });
    let gave_up = lost.gave_up;
    tracing::warn!(%thread, gave_up, live = live.is_some(), "the agent lost its connection");
    if let Some(turn) = live {
        let note = lost.title.clone().unwrap_or_else(|| LOST_DETAIL.to_owned());
        arm_note(actor, thread, turn, note, None);
    }
}

/// Arms (or arms again) the watch of `thread`'s `turn` on an agent's note, holding `held`.
fn arm_note(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    note: String,
    held: Option<(AgentSessionId, AgentUpdate)>,
) {
    let now = actor.now().mono;
    let Some(current) = current(actor, thread).filter(|current| current.turn == turn) else {
        return;
    };
    let watch = current.offline.get_or_insert_with(|| Watch {
        since: now,
        first: now,
        note: None,
        held: Vec::new(),
        probing: false,
    });
    watch.since = now;
    watch.note = Some(note);
    watch.held.extend(held);
    arm_timer(actor, thread, turn);
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
    // The end's first call decided; an agent's exit while the turn ends changes nothing.
    if current.phase == TurnPhase::Ending {
        return;
    }
    let failure = failure.or_else(|| current.failure.as_ref().map(|(code, _)| *code));
    if failure == Some(ErrorCode::AgentOffline) {
        current.offline = None;
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
        first: now,
        note: None,
        held: Vec::new(),
        probing: false,
    });
    let turn = current.turn;
    arm_timer(actor, thread, turn);
}

/// The watch's wait ran out, and the agent sent nothing since (nor did a later note arm it
/// again). A turn waiting on the lawyer (a permission, an elicitation), or running a tool call
/// under a watch only the window armed and younger than [`WORKING_LIMIT`], is quiet, not cut
/// off: it waits again. Else the host's probe decides ([`on_probed`]): the turn is cancelled
/// through the Cancel path and ends `Failed { agent.offline }` only if it cannot get through.
pub(super) fn on_timer(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let now = actor.now().mono;
    let asking = updates::agent(actor, thread).is_some_and(|agent| {
        !(agent.permissions.is_empty()
            && agent.elicitations.is_empty()
            && agent.url_elicitations.is_empty())
    });
    let working = actor
        .loaded(StreamId::Thread(thread))
        .is_some_and(|loaded| loaded.sent.unfinished_tools(turn).next().is_some());
    let Some(watch) = live_watch(actor, thread, turn)
        .filter(|watch| now.saturating_sub(watch.since) >= OFFLINE_WAIT)
    else {
        return;
    };
    // A note means the model call failed, so a tool call still open then is stale; under a
    // window-only watch one keeps the turn going up to [`WORKING_LIMIT`], then the probe decides.
    let noted = watch.note.is_some();
    let working = working && !noted && now.saturating_sub(watch.first) < WORKING_LIMIT;
    if asking || working {
        tracing::info!(%thread, asking, working, "the agent is quiet but at work; waiting again");
        watch.since = now;
        arm_timer(actor, thread, turn);
    } else if !watch.probing {
        watch.probing = true;
        probe::start(actor, thread, turn);
    }
}

/// The host's probe for `thread`'s `turn` answered: `reached` the agent's service (or its
/// proxy), so the window is wrong, or the agent's retry (Codex's HTTPS after its WebSocket
/// failed) can get through, and the watch waits again; else the turn ends offline. A watch
/// that went ignores it.
pub(super) fn on_probed(actor: &mut Actor, thread: ThreadId, turn: TurnId, reached: bool) {
    let now = actor.now().mono;
    let Some(watch) = live_watch(actor, thread, turn).filter(|watch| watch.probing) else {
        return;
    };
    watch.probing = false;
    let noted = watch.note.is_some();
    tracing::info!(%thread, reached, noted, "the host's probe of the agent's service");
    if reached {
        watch.since = now;
        arm_timer(actor, thread, turn);
    } else {
        end_offline(actor, thread);
    }
}

/// The watch of `thread`'s `turn`, while it is live and has no failure.
fn live_watch(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> Option<&mut Watch> {
    current(actor, thread)
        .filter(|current| current.turn == turn && current.live() && current.failure.is_none())
        .and_then(|current| current.offline.as_mut())
}

/// Cancels `thread`'s live turn with `agent.offline` as its failure; its watch goes.
fn end_offline(actor: &mut Actor, thread: ThreadId) {
    let Some(current) = current(actor, thread) else {
        return;
    };
    let note = current.offline.take().and_then(|watch| watch.note);
    let by = if note.is_some() {
        "agent_note"
    } else {
        "window"
    };
    tracing::warn!(%thread, by, "the agent went quiet while offline; ending the turn");
    let detail = note.unwrap_or_else(|| NETWORK_DETAIL.to_owned());
    current.failure = Some((ErrorCode::AgentOffline, Some(detail)));
    cancel::cancel_running(actor, thread, None, cancel::why::FAILURE);
}

/// The code a failed prompt of `thread` ends with when it is offline. A sign-in the error asks
/// for comes first, and an error that says a server answered (a status, a limit) is never
/// offline, watch or not; else the error names a connection failure, or an agent's note armed
/// the watch.
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
    // Only an agent's note counts: the window's word alone does not make any failure offline.
    let noted = current(actor, thread)
        .and_then(|current| current.offline.as_ref())
        .is_some_and(|watch| watch.note.is_some());
    (!server && (noted || connection)).then_some(ErrorCode::AgentOffline)
}

/// The code a prompt of `thread` that ended carrying a `connection` failure (codex-acp's, once
/// Codex's retries ran out) ends with: `agent.offline`, unless its title says a server answered,
/// when an agent's note armed the watch or the title names a connection failure. Else the
/// turn ends as the agent said (codex-acp's own "Connection to Codex was lost." when its
/// app-server exits names none).
pub(super) fn connection_ended_offline(
    actor: &mut Actor,
    thread: ThreadId,
    lost: &ConnectionLost,
) -> Option<ErrorCode> {
    let title = lost.title.as_deref().unwrap_or_default();
    let noted = current(actor, thread)
        .and_then(|current| current.offline.as_ref())
        .is_some_and(|watch| watch.note.is_some());
    let offline = !classify::says_server(title) && (noted || classify::says_connection(title));
    tracing::info!(%thread, offline, noted, "the prompt ended on a lost connection");
    offline.then_some(ErrorCode::AgentOffline)
}

/// How a prompt of `thread` that ended `result` fails for its connection: `(offline, lost)`. An
/// error is offline by [`prompt_failed_offline`]. An answer that carried a `connection` failure
/// fails offline by [`connection_ended_offline`], its title the detail. A turn that ends
/// `TooLong` (`too_long`) does neither.
pub(super) fn prompt_ended(
    actor: &mut Actor,
    thread: ThreadId,
    result: &Result<PromptEnd, clauseo_acp::Error>,
    too_long: bool,
) -> (Option<ErrorCode>, Option<(ErrorCode, Option<String>)>) {
    match result {
        _ if too_long => (None, None),
        Err(error) => (prompt_failed_offline(actor, thread, error), None),
        Ok(PromptEnd {
            connection: Some(lost),
            ..
        }) => {
            let code = connection_ended_offline(actor, thread, lost);
            (None, code.map(|code| (code, lost.title.clone())))
        }
        Ok(_) => (None, None),
    }
}

#[cfg(test)]
mod tests;
