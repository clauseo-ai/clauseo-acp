//! A steer Claude accepted, until it is seen taken in. claude-agent-acp answers `injected` as
//! soon as the steer is in its own queue, not when the model reads it, and a cancel drops it
//! unread, so for an agent with a Claude home that answer leaves the steer accepted and
//! unconfirmed, with nothing logged yet. The transcript confirms it (`transcript.rs`), read off
//! the actor from where it stood when the steer went out: every [`EVERY`] while the steer's turn
//! runs, at once when that turn ends, then every [`EVERY`] for [`GRACE`] more, as Claude Code
//! may write the line late or run the queued steer as a turn of its own once the turn has ended
//! (`own_turn.rs`). After a cancel, which drops the steer, the check at the end is the last. A
//! last check whose read stops at its cap reads on at once, to 16 MiB in all, before it settles.
//! Confirmed, it is logged `SteerResolved { Injected }` with the tool calls that were running
//! when the agent answered; still unconfirmed at the last check, `SteerResolved { NotSent }`,
//! and it is never sent on its own: the lawyer sends it again (Send now on it requeues it,
//! `steer::send_now`). Nothing in the queue waits for it meanwhile. A Claude agent whose
//! transcript cannot be placed (no home, no session) has nothing to read: its steer is
//! `NotSent` as soon as its turn ends, with no check and no timer. Any other agent with no Claude
//! home (Codex, which never steers for real, and the tests' fake agent) has no better signal, so
//! its `injected` counts as confirmed at once. One check runs at a time per thread, every timer
//! on the host clock; its answer arms the next. Each is logged at info on the lifecycle target,
//! with no text.

use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{
    AgentKind, ContentBlock, MessageId, SteerOutcome, SteerResolved, StopReason, StreamId,
    ThreadId, ToolCallId, TurnId,
};
use futures::future::AbortHandle;

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::out::Out;
use super::super::{Then, TimerKind, TurnInput, TurnTimer, state};
use super::queue;
use super::steer::{Ask, why};
use super::transcript::{self, Place, Seen, Session, Wanted};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

/// How often the transcript is read while an unconfirmed steer waits.
const EVERY: Duration = Duration::from_secs(2);
/// How long after its turn's end, whatever its stop but a cancel, a steer may still be seen
/// taken in.
const GRACE: Duration = Duration::from_secs(10);
/// How many more reads, each at its cap, a steer's last check may take to reach the end of a
/// transcript before it settles `NotSent`: 16 MiB in all.
const LAST_READS: u8 = 7;
/// A check, waited on for `PREPARE_WAIT`; past it the steers it looked for are unconfirmed.
const JOB: Job = Job::short("steer check", None);

/// A thread's steers accepted and not confirmed yet, and their check.
#[derive(Debug, Default)]
pub(crate) struct Confirms {
    accepted: Vec<Accepted>,
    /// The next check.
    timer: Option<AbortHandle>,
    /// A check is out; its answer arms the next.
    checking: bool,
}

/// A steer Claude answered `injected`, not seen taken in yet.
#[derive(Debug)]
struct Accepted {
    ask: Ask,
    /// The steered turn's tool calls not finished when the agent answered.
    aside: Vec<ToolCallId>,
    /// Its transcripts and where the next check reads each from; none when they are unknown.
    places: Vec<Place>,
    /// Its turn has ended: checks go on until this time on the host clock's monotonic scale, and
    /// the first one then is its last.
    until: Option<Duration>,
    /// The next check is due now: its turn has just ended, or a read stopped at its cap.
    due: bool,
    /// The further reads its last check took, each stopped at its cap ([`LAST_READS`]).
    last_reads: u8,
}

/// A check's answer: for each steer it looked for, what it saw and whether this was its last
/// check.
#[derive(Debug)]
pub(crate) struct Checked {
    thread: ThreadId,
    found: Vec<(MessageId, Seen, bool)>,
}

/// Where `thread`'s agent writes its transcript: a Claude agent's home, folder and session.
fn session_of(actor: &mut Actor, thread: ThreadId) -> Option<Session> {
    let agent = state(actor, thread)?.agent.as_deref()?;
    Some(Session {
        home: agent.claude_home.clone()?,
        folder: agent.folder.clone(),
        id: agent.session.clone()?,
    })
}

/// `thread`'s transcripts, marked where they stand now, just before a steer goes out
/// (`transcript::mark`); none when its agent has no Claude home or no session yet.
pub(super) fn mark(actor: &mut Actor, thread: ThreadId) -> Vec<Place> {
    session_of(actor, thread).map_or_else(Vec::new, |session| transcript::mark(&session))
}

/// Whether `thread` runs Claude, by its agent's id: a steer it accepts waits for its transcript
/// even when that cannot be placed.
fn claude(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .is_some_and(|entry| AgentKind::of(&entry.row.agent) == Some(AgentKind::Claude))
}

/// The agent answered `injected` to `ask`, with `aside` running. A Claude agent's steer waits
/// for its transcript, read from the places marked at its send; any other's is logged
/// `Injected` now. A steer whose turn has already ended (it was answered after `EndTurn`) has
/// [`GRACE`] from now.
pub(super) fn accepted(actor: &mut Actor, thread: ThreadId, mut ask: Ask, aside: Vec<ToolCallId>) {
    if session_of(actor, thread).is_none() && !claude(actor, thread) {
        log(actor, thread, injected(&ask, aside));
        return;
    }
    let now = actor.now().mono;
    let ended = state(actor, thread).is_none_or(|state| {
        state
            .current
            .as_ref()
            .is_none_or(|current| current.turn != ask.turn)
    });
    let places = std::mem::take(&mut ask.places);
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    confirms.accepted.push(Accepted {
        ask,
        aside,
        places,
        until: ended.then(|| now.saturating_add(GRACE)),
        due: ended,
        last_reads: 0,
    });
    settle_unplaced(actor, thread);
    schedule(actor, thread);
}

fn confirms(actor: &mut Actor, thread: ThreadId) -> Option<&mut Confirms> {
    queue(actor, thread).map(|queue| &mut queue.confirms)
}

/// `turn` has ended with `stop`: its steers are checked now, then for [`GRACE`], or only now
/// after a cancel.
pub(super) fn turn_ended(actor: &mut Actor, thread: ThreadId, turn: TurnId, stop: StopReason) {
    let now = actor.now().mono;
    let until = if stop == StopReason::Cancelled {
        now
    } else {
        now.saturating_add(GRACE)
    };
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    let mut any = false;
    for accepted in confirms
        .accepted
        .iter_mut()
        .filter(|accepted| accepted.ask.turn == turn && accepted.until.is_none())
    {
        accepted.until = Some(until);
        accepted.due = true;
        any = true;
    }
    if any {
        settle_unplaced(actor, thread);
        schedule(actor, thread);
    }
}

/// Each steer with no transcript to read whose turn has ended is `NotSent` now: nothing could
/// ever confirm it.
fn settle_unplaced(actor: &mut Actor, thread: ThreadId) {
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    let unplaced: Vec<Ask> = confirms
        .accepted
        .extract_if(.., |accepted| {
            accepted.places.is_empty() && accepted.until.is_some()
        })
        .map(|accepted| accepted.ask)
        .collect();
    for ask in &unplaced {
        not_sent(actor, thread, ask, why::NO_CONFIRMATION);
    }
}

/// Whether a steer sent into `turn` waits for its confirmation.
pub(super) fn waits(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    confirms(actor, thread).is_some_and(|confirms| {
        confirms
            .accepted
            .iter()
            .any(|accepted| accepted.ask.turn == turn)
    })
}

/// Takes out the steers of `turn` still unconfirmed, for Send now; a later check ignores them.
pub(super) fn take(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> Vec<Ask> {
    let Some(confirms) = confirms(actor, thread) else {
        return Vec::new();
    };
    let taken = confirms
        .accepted
        .extract_if(.., |accepted| accepted.ask.turn == turn)
        .map(|accepted| accepted.ask)
        .collect();
    if !confirms.looks()
        && let Some(timer) = confirms.timer.take()
    {
        timer.abort();
    }
    taken
}

impl Confirms {
    /// Whether a steer has a transcript to read.
    fn looks(&self) -> bool {
        self.accepted
            .iter()
            .any(|accepted| !accepted.places.is_empty())
    }
}

/// Checks now when a steer's check is due, else arms the next check while any steer has a
/// transcript to read: in [`EVERY`], or sooner when a steer's last check comes first. Nothing
/// while a check is out, whose answer schedules again.
fn schedule(actor: &mut Actor, thread: ThreadId) {
    let now = actor.now().mono;
    let Some(waiting) = confirms(actor, thread) else {
        return;
    };
    if waiting.checking || !waiting.looks() {
        return;
    }
    let looking = || {
        waiting
            .accepted
            .iter()
            .filter(|accepted| !accepted.places.is_empty())
    };
    let last = looking()
        .filter_map(|accepted| accepted.until)
        .min()
        .map(|until| until.saturating_sub(now));
    if looking().any(|accepted| accepted.due) || last == Some(Duration::ZERO) {
        if let Some(timer) = waiting.timer.take() {
            timer.abort();
        }
        check(actor, thread);
    } else if waiting.timer.is_none() {
        let delay = last.map_or(EVERY, |last| last.min(EVERY));
        let timer = actor.arm_timer(
            delay,
            TurnTimer {
                thread,
                kind: TimerKind::SteerCheck,
            },
        );
        if let Some(confirms) = confirms(actor, thread) {
            confirms.timer = Some(timer);
        }
    }
}

/// The check timer fired.
pub(in super::super) fn on_timer(actor: &mut Actor, thread: ThreadId) {
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    confirms.timer = None;
    if !confirms.checking && confirms.looks() {
        check(actor, thread);
    }
}

/// Reads the transcripts of every unconfirmed steer of `thread` that has one, off the actor,
/// each from where the last check stopped. A steer whose turn ended no later than [`GRACE`]
/// ago (none after a cancel) gets its last check.
fn check(actor: &mut Actor, thread: ThreadId) {
    let clock = Arc::clone(&actor.clock);
    let now = actor.now().mono;
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    confirms.checking = true;
    let mut asked = Vec::new();
    let mut work = Vec::new();
    for accepted in confirms
        .accepted
        .iter_mut()
        .filter(|accepted| !accepted.places.is_empty())
    {
        accepted.due = false;
        let texts = accepted.ask.content.iter().filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        });
        let last = accepted.until.is_some_and(|until| now >= until);
        asked.push((accepted.ask.message, accepted.places.clone(), last));
        work.push((
            accepted.places.clone(),
            Wanted::new(texts, accepted.ask.sent),
        ));
    }
    actor.spawn_post(async move {
        let mut seen = off_actor(clock, JOB, move || Ok(transcript::taken(work)))
            .await
            .unwrap_or_default()
            .into_iter();
        let found = asked
            .into_iter()
            .map(|(message, places, last)| {
                // A check that did not answer saw nothing, and leaves the places as they were.
                let seen = seen.next().unwrap_or(Seen {
                    found: false,
                    places,
                    behind: false,
                });
                (message, seen, last)
            })
            .collect();
        Input::Turn(TurnInput::SteerChecked(Box::new(Checked { thread, found })))
    });
}

/// A check answered: each steer found is logged `Injected`; each not found at its last check,
/// `NotSent`, unless a read stopped at its cap with more to read, when the last check reads on
/// at once, up to [`LAST_READS`] times; every other reads on from where this check stopped, at
/// once when a read stopped at its cap. Then the next check is scheduled.
pub(in super::super) fn on_checked(actor: &mut Actor, checked: Checked) {
    let Checked { thread, found } = checked;
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    confirms.checking = false;
    let mut settled = Vec::new();
    for (message, seen, last) in found {
        let at = confirms
            .accepted
            .iter()
            .position(|accepted| accepted.ask.message == message);
        let Some(at) = at else {
            continue;
        };
        let reads_on = confirms
            .accepted
            .get(at)
            .is_some_and(|accepted| seen.behind && accepted.last_reads < LAST_READS);
        if seen.found || (last && !reads_on) {
            settled.push((confirms.accepted.remove(at), seen.found));
        } else if let Some(accepted) = confirms.accepted.get_mut(at) {
            accepted.places = seen.places;
            accepted.due |= seen.behind;
            accepted.last_reads += u8::from(last);
        }
    }
    for (accepted, found) in settled {
        let Accepted { ask, aside, .. } = accepted;
        if found {
            tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, "a steer was confirmed");
            log(actor, thread, injected(&ask, aside));
        } else {
            not_sent(actor, thread, &ask, why::NO_CONFIRMATION);
        }
    }
    schedule(actor, thread);
}

/// `SteerResolved { Injected }` of `ask` with `aside`.
fn injected(ask: &Ask, aside: Vec<ToolCallId>) -> SteerResolved {
    SteerResolved {
        message: ask.message,
        outcome: SteerOutcome::Injected { turn: ask.turn },
        aside,
    }
}

/// `ask` was not sent: `SteerResolved { NotSent }` with its turn, for `why`.
pub(super) fn not_sent(actor: &mut Actor, thread: ThreadId, ask: &Ask, why: &'static str) {
    tracing::info!(target: LIFECYCLE, %thread, turn = %ask.turn, why, "a steer was not sent");
    let resolved = SteerResolved {
        message: ask.message,
        outcome: SteerOutcome::NotSent { turn: ask.turn },
        aside: Vec::new(),
    };
    log(actor, thread, resolved);
}

/// Logs `resolved` once `thread`'s stream is loaded; a failure is a warning.
fn log(actor: &mut Actor, thread: ThreadId, resolved: SteerResolved) {
    let stream = StreamId::Thread(thread);
    actor.with_loaded(
        stream,
        Box::new(move |actor: &mut Actor, loaded: Result<(), Error>| {
            let logged = loaded.and_then(|()| {
                let mut out = Out::new(actor, thread, None)?;
                out.push(actor, resolved)?;
                out.send(actor, Then::Nothing)
            });
            if let Err(error) = logged {
                tracing::warn!(%thread, error = %Cause(&error), "a steer's outcome was not logged");
            }
            actor.release(stream);
        }),
    );
}
