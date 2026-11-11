//! A steer Claude accepted, until it is seen taken in. claude-agent-acp answers `injected` as
//! soon as the steer is in its own queue, not when the model reads it, and a cancel drops it
//! unread, so for an agent with a Claude home that answer leaves the steer accepted and
//! unconfirmed, with nothing logged yet. The transcript confirms it (`transcript.rs`): read off
//! the actor every [`EVERY`] while the steer's turn runs, and once more once that turn has ended,
//! whatever its stop. Confirmed, it is logged `SteerResolved { Injected }` with the tool calls
//! that were running when the agent answered; still unconfirmed at the check after its turn's
//! end, `SteerResolved { NotSent }`, and it is never sent on its own: the lawyer sends it again
//! (Send now on it requeues it, `steer::send_now`). An agent with no Claude home (Codex, which
//! never steers for real, and the tests' fake agent) has no better signal, so its `injected`
//! counts as confirmed at once. One check runs at a time per thread; its answer arms the next.
//! Each is logged at info on the lifecycle target, with no text.

use std::sync::Arc;
use std::time::Duration;

use clauseo_protocol::{
    ContentBlock, MessageId, SteerOutcome, SteerResolved, StreamId, ThreadId, ToolCallId, TurnId,
};
use futures::future::AbortHandle;

use super::super::super::Actor;
use super::super::super::input::Input;
use super::super::out::Out;
use super::super::{Then, TimerKind, TurnInput, TurnTimer, state};
use super::queue;
use super::steer::{Ask, why};
use super::transcript::{self, Session, Wanted};
use crate::agent::codex::prepare::{Job, off_actor};
use crate::error::Error;
use crate::log_file::{Cause, LIFECYCLE};

/// How often the transcript is read while an unconfirmed steer's turn runs.
const EVERY: Duration = Duration::from_secs(2);
/// A check, waited on for `PREPARE_WAIT`; past it the steers it looked for are unconfirmed.
const JOB: Job = Job::short("steer check", None);

/// A thread's steers accepted and not confirmed yet, and their check.
#[derive(Debug, Default)]
pub(crate) struct Confirms {
    accepted: Vec<Accepted>,
    /// The next check while a turn runs.
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
    session: Session,
    /// Its turn has ended: the next check is its last.
    ended: bool,
}

/// A check's answer: for each steer it looked for, whether it was found, and whether this was
/// its last check.
#[derive(Debug)]
pub(crate) struct Checked {
    thread: ThreadId,
    found: Vec<(MessageId, bool, bool)>,
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

/// The agent answered `injected` to `ask`, with `aside` running. A Claude agent's steer waits
/// for its transcript; any other's is logged `Injected` now.
pub(super) fn accepted(actor: &mut Actor, thread: ThreadId, ask: Ask, aside: Vec<ToolCallId>) {
    let Some(session) = session_of(actor, thread) else {
        log(actor, thread, injected(&ask, aside));
        return;
    };
    let ended = state(actor, thread).is_none_or(|state| {
        state
            .current
            .as_ref()
            .is_none_or(|current| current.turn != ask.turn)
    });
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    confirms.accepted.push(Accepted {
        ask,
        aside,
        session,
        ended,
    });
    schedule(actor, thread);
}

fn confirms(actor: &mut Actor, thread: ThreadId) -> Option<&mut Confirms> {
    queue(actor, thread).map(|queue| &mut queue.confirms)
}

/// `turn` has ended, whatever its stop: the check that follows is the last of its steers.
pub(super) fn turn_ended(actor: &mut Actor, thread: ThreadId, turn: TurnId) {
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    let mut any = false;
    for accepted in confirms
        .accepted
        .iter_mut()
        .filter(|accepted| accepted.ask.turn == turn)
    {
        accepted.ended = true;
        any = true;
    }
    if any {
        schedule(actor, thread);
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
    if confirms.accepted.is_empty()
        && let Some(timer) = confirms.timer.take()
    {
        timer.abort();
    }
    taken
}

/// Checks now when a steer's turn has ended, else arms the next check while any waits; nothing
/// while a check is out, whose answer schedules again.
fn schedule(actor: &mut Actor, thread: ThreadId) {
    let Some(waiting) = confirms(actor, thread) else {
        return;
    };
    if waiting.checking || waiting.accepted.is_empty() {
        return;
    }
    if waiting.accepted.iter().any(|accepted| accepted.ended) {
        if let Some(timer) = waiting.timer.take() {
            timer.abort();
        }
        check(actor, thread);
    } else if waiting.timer.is_none() {
        let timer = actor.arm_timer(
            EVERY,
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
    if !confirms.checking && !confirms.accepted.is_empty() {
        check(actor, thread);
    }
}

/// Reads the transcript for every unconfirmed steer of `thread`, off the actor.
fn check(actor: &mut Actor, thread: ThreadId) {
    let clock = Arc::clone(&actor.clock);
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    confirms.checking = true;
    let mut asked = Vec::new();
    let mut work = Vec::new();
    for accepted in &confirms.accepted {
        let texts = accepted.ask.content.iter().filter_map(|block| {
            if let ContentBlock::Text { text } = block {
                Some(text.as_str())
            } else {
                None
            }
        });
        asked.push((accepted.ask.message, accepted.ended));
        work.push((
            accepted.session.clone(),
            Wanted::new(texts, accepted.ask.sent),
        ));
    }
    actor.spawn_post(async move {
        let found = off_actor(clock, JOB, move || Ok(transcript::taken(&work)))
            .await
            .unwrap_or_default();
        let found = asked
            .into_iter()
            .enumerate()
            .map(|(at, (message, last))| (message, found.get(at).copied().unwrap_or(false), last))
            .collect();
        Input::Turn(TurnInput::SteerChecked(Box::new(Checked { thread, found })))
    });
}

/// A check answered: each steer found is logged `Injected`; each not found whose turn had ended
/// when the check went out, `NotSent`. Then the next check is scheduled.
pub(in super::super) fn on_checked(actor: &mut Actor, checked: Checked) {
    let Checked { thread, found } = checked;
    let Some(confirms) = confirms(actor, thread) else {
        return;
    };
    confirms.checking = false;
    let mut settled = Vec::new();
    for (message, found, last) in found {
        if !(found || last) {
            continue;
        }
        let at = confirms
            .accepted
            .iter()
            .position(|accepted| accepted.ask.message == message);
        if let Some(at) = at {
            settled.push((confirms.accepted.remove(at), found));
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
