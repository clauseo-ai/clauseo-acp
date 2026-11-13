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
//! - Silence: 30 s with no model output, no open tool call and no ask (`quiet.rs`), for an
//!   agent whose retries reach the client as nothing (Claude), or as notes that say a server
//!   answered (Codex behind a proxy that refuses the tunnel); 5 minutes end the turn
//!   `agent.quiet` whatever the probe says.
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
//! one that ends on a `connection` failure the agent gave up on, unless the host's last probe
//! got through (then it fails `agent.protocol_error`). The log gets fixed fields only, never
//! the agent's text (decision 0047).

use std::time::Duration;

use clauseo_acp::{AgentUpdate, ConnectionLost, MessageRole, Origin};
use clauseo_protocol::{
    AgentSessionId, CommandOutcome, ContentBlock, ErrorCode, StreamId, ThreadId, TurnId,
};

use super::super::Actor;
use super::super::commands::CommandCtx;
use super::{TimerKind, TurnPhase, TurnTimer, cancel, current, updates};

pub(super) mod classify;
mod ended;
pub(crate) mod probe;
pub(super) mod quiet;

pub(super) use ended::prompt_ended;
pub(super) use quiet::ask_ended;

use probe::{Reach, ReachClass};

/// How long a turn waits after the agent said its connection failed, or the window said the
/// computer went offline, for any sign the agent got through, before it ends offline.
pub(super) const OFFLINE_WAIT: Duration = Duration::from_secs(30);

/// How long a watch only the window armed lets a tool call still running keep the turn going:
/// past it, a probe that cannot reach the agent's service ends the turn offline anyway.
pub(super) const WORKING_LIMIT: Duration = Duration::from_secs(600);

/// The failure's detail when the window's report armed the watch.
const NETWORK_DETAIL: &str = "the window said the computer is offline, the agent sent \
                              nothing for 30 s and the host's probe did not get through";

/// The failure's detail when the quiet arm armed the watch (`quiet.rs`).
const QUIET_DETAIL: &str = "the agent sent nothing for 30 s with no tool running, and the host's \
                            probe of its service did not get through twice";

/// The note a `connection` failure without a title arms the watch with.
const LOST_DETAIL: &str = "the agent said it lost its connection to its service";

/// What armed a watch last.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Arm {
    /// The window said offline.
    Window,
    /// An agent's note about its connection, or a `connection` failure.
    Note,
    /// The agent sent nothing for [`quiet::QUIET_WAIT`] (`quiet.rs`).
    Quiet,
}

impl Arm {
    /// Its word in the log.
    const fn as_str(self) -> &'static str {
        match self {
            Self::Window => "window",
            Self::Note => "note",
            Self::Quiet => "quiet",
        }
    }
}

/// The watch on the live turn.
#[derive(Debug)]
pub(super) struct Watch {
    /// What armed it last: a note takes over a quiet or window watch, the window a quiet one.
    by: Arm,
    /// The host's probes for it in a row that did not get through.
    misses: u8,
    /// The class of the host's last probe for it, once one answered.
    class: Option<ReachClass>,
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
    /// The host's last probe for it got through: the agent's service (or its proxy) takes
    /// connections, so a prompt that then ends on a lost connection is not offline. The next
    /// probe overwrites it; the watch's end drops it.
    reached: bool,
}

impl Watch {
    /// A watch armed `by` at `now`.
    const fn new(now: Duration, by: Arm) -> Self {
        Self {
            by,
            misses: 0,
            class: None,
            since: now,
            first: now,
            note: None,
            held: Vec::new(),
            probing: false,
            reached: false,
        }
    }
}

#[cfg(test)]
impl Watch {
    /// What armed it last.
    pub(super) const fn by(&self) -> Arm {
        self.by
    }

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

    /// Whether the host's last probe for it got through.
    pub(super) const fn reached(&self) -> bool {
        self.reached
    }

    /// The class of the host's last probe for it.
    pub(super) const fn class(&self) -> Option<ReachClass> {
        self.class
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

/// Whether an update outside a turn (idle, or replayed) is Codex's note about its connection
/// in a notice, which never reaches the transcript, turn or not. Answer text outside a turn is
/// never touched: a replay sends whole messages, and one may start with the note's words.
pub(super) fn outside_note(thread: ThreadId, update: &AgentUpdate) -> bool {
    let AgentUpdate::Notice { text, .. } = update else {
        return false;
    };
    let sure = classify::transport_note(text).is_some_and(|note| note.sure);
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

/// Arms the watch of `thread`'s `turn` on an agent's note, holding `held`. A watch that already
/// has a note only takes the new one: its wait runs from the first note, so retries less than
/// 30 s apart (Codex's) never put the probe off. A watch only the window armed waits from the
/// note. A quiet watch keeps its schedule: after a missed probe the next miss ends it, as a
/// note's watch ends on one.
fn arm_note(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    note: String,
    held: Option<(AgentSessionId, AgentUpdate)>,
) {
    let now = actor.now().mono;
    let Some(current) =
        current(actor, thread).filter(|current| current.turn == turn && current.failure.is_none())
    else {
        return;
    };
    let watch = current
        .offline
        .get_or_insert_with(|| Watch::new(now, Arm::Note));
    let quiet = watch.by == Arm::Quiet;
    watch.by = Arm::Note;
    let noted = watch.note.replace(note).is_some();
    watch.held.extend(held);
    if !noted && !quiet {
        watch.since = now;
        arm_timer(actor, thread, turn);
    }
}

/// The agent of `turn` is at work (an answer, a tool call, a permission request, a limit): the
/// watch goes, and the lines it held go back into the answer, in order.
pub(super) fn agent_active(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    quiet::output(actor, thread, turn);
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
/// every running turn (a turn that starts later gets it from [`turn_started`]), and a quiet
/// watch becomes the window's, its schedule kept; online takes back each watch it armed last,
/// never one an agent's note or the quiet arm armed. Answers `Done`, no receipt.
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
                .is_some_and(|watch| watch.by == Arm::Window)
            {
                current.offline = None;
            }
        } else {
            network_watch(actor, thread);
        }
    }
    actor.finish_command(ctx.id, Ok(CommandOutcome::Done));
}

/// `thread`'s turn went live (its prompt is out): while the window says offline, it is watched,
/// and the quiet arm's clock starts (`quiet.rs`).
pub(super) fn turn_started(actor: &mut Actor, thread: ThreadId) {
    quiet::turn_started(actor, thread);
    if actor.turn_path.offline {
        network_watch(actor, thread);
    }
}

/// Arms the window's watch on `thread`'s live turn, unless one stands already; a quiet one
/// becomes the window's.
fn network_watch(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    let Some(current) =
        current(actor, thread).filter(|current| current.live() && current.failure.is_none())
    else {
        return;
    };
    if let Some(watch) = current.offline.as_mut() {
        if watch.by == Arm::Quiet {
            watch.by = Arm::Window;
        }
        return;
    }
    current.offline = Some(Watch::new(now, Arm::Window));
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
    let (asking, working) = at_work(actor, thread, turn);
    let Some(watch) = live_watch(actor, thread, turn)
        .filter(|watch| now.saturating_sub(watch.since) >= OFFLINE_WAIT)
    else {
        return;
    };
    // A quiet watch while the agent waits on the lawyer or runs a tool is no longer quiet: it
    // goes, and the quiet arm's own timer takes over.
    if watch.by == Arm::Quiet && (asking || working) {
        if let Some(current) = current(actor, thread) {
            current.offline = None;
        }
        return;
    }
    // A note means the model call failed, so a tool call still open then is stale; under a
    // window-only watch one keeps the turn going up to [`WORKING_LIMIT`], then the probe decides.
    let noted = watch.by == Arm::Note;
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

/// Whether `thread`'s agent waits on the lawyer (a permission, an elicitation), and whether a
/// tool call of `turn` is still open.
pub(super) fn at_work(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> (bool, bool) {
    let asking = updates::agent(actor, thread).is_some_and(|agent| {
        !(agent.permissions.is_empty()
            && agent.elicitations.is_empty()
            && agent.url_elicitations.is_empty())
    });
    let working = actor
        .loaded(StreamId::Thread(thread))
        .is_some_and(|loaded| loaded.sent.unfinished_tools(turn).next().is_some());
    (asking, working)
}

/// The host's probe for `thread`'s `turn` answered: it `reached` the agent's service (or its
/// proxy), so the window is wrong, the agent's retry (Codex's HTTPS after its WebSocket failed)
/// can get through, or a quiet agent is thinking, and the watch waits again. Else the turn ends
/// offline, a quiet watch only at its [`quiet::QUIET_MISSES`]th miss in a row. A launch with
/// nothing to probe (`unknown_service`, `unknown_route`) counts as reached for a quiet or a
/// note's watch, but as a miss for the window's: the OS says offline and nothing says otherwise.
/// A watch that went ignores it. The line goes to host.log (the lifecycle target).
pub(super) fn on_probed(actor: &mut Actor, thread: ThreadId, turn: TurnId, reach: Reach) {
    let now = actor.now().mono;
    let Some(watch) = live_watch(actor, thread, turn).filter(|watch| watch.probing) else {
        return;
    };
    let reached = reach.reached() && !(watch.by == Arm::Window && reach.class.unknown());
    watch.probing = false;
    watch.reached = reached;
    watch.class = Some(reach.class);
    watch.misses = if reached {
        0
    } else {
        watch.misses.saturating_add(1)
    };
    let (by, misses) = (watch.by, watch.misses);
    probe::log(
        thread,
        (reach, reached),
        by == Arm::Note,
        by.as_str(),
        misses,
    );
    if reached || (by == Arm::Quiet && misses < quiet::QUIET_MISSES) {
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
    let Some(watch) = current.offline.take() else {
        return;
    };
    let (by, detail) = match (watch.by, watch.note) {
        (Arm::Note, Some(note)) => ("agent_note", note),
        (Arm::Quiet, _) => ("quiet", QUIET_DETAIL.to_owned()),
        (Arm::Window | Arm::Note, _) => ("window", NETWORK_DETAIL.to_owned()),
    };
    tracing::warn!(%thread, by, "the agent went quiet while offline; ending the turn");
    current.failure = Some((ErrorCode::AgentOffline, Some(detail)));
    cancel::cancel_running(actor, thread, None, cancel::why::FAILURE);
}

#[cfg(test)]
mod tests;
