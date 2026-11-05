//! The snooze timer: a snooze wakes at its time with no client timer. The host arms one timer
//! on its clock for the earliest `snoozed_until` among the headers it holds (threads of listed
//! matters), and when it fires logs `thread_snoozed { by: expired }` (no `until`) for every
//! thread whose time has come, then arms for the next. The view needs no clock: the wake is a
//! logged event, so the header and the thread's row change as for any other.
//!
//! - The timer is re-armed after any input that may have moved the earliest time ([`tick`],
//!   from the run loop): a returned commit holding a `thread_snoozed` or an event that wakes a
//!   thread (`clauseo_reducer::wakes_thread`), and a thread stream loaded (its header is known
//!   from then on, which is how a snooze that ended while the host was down, or its matter was
//!   off the list, wakes at start-up or import: its time has passed, so the timer fires at once).
//! - A thread whose stream has a commit in flight is passed over until a commit returns; one
//!   whose stream is blocked until it is written again.
//! - The timer counts on monotonic time, which stops while the computer sleeps, so every tick
//!   first fires an armed timer whose wall time has passed.
//! - One wake runs at a time: the threads are loaded ([`load_all`]), checked again on their
//!   loaded state and woken in one commit; the next timer is armed when it returns.

use std::collections::BTreeMap;
use std::time::Duration;

use clauseo_protocol::{SnoozeBy, StreamId, ThreadId, ThreadSnoozed, Timestamp};
use futures::future::AbortHandle;

use super::{Actor, facts, load_all};
use crate::actor::input::Input;
use crate::actor::{HostPhase, Tag};
use crate::pipeline::{Draft, Failure};

/// The triage's in-memory state: streams held loaded, and the snooze timer.
#[derive(Default)]
pub(crate) struct Triage {
    /// Threads a `load_all` holds loaded, with how many hold each.
    pub(super) held: BTreeMap<ThreadId, usize>,
    /// The armed timer and the wall time it is for.
    timer: Option<(AbortHandle, Timestamp)>,
    /// Moves on each time the timer is taken, so a fire already posted is known stale.
    generation: u64,
    /// The earliest snooze may have moved: re-arm at the next tick.
    dirty: bool,
    /// A snoozed thread was passed over for a commit in flight: re-arm when a commit returns.
    recheck: bool,
    /// A wake is loading or committing.
    busy: bool,
}

impl Triage {
    /// A triage that arms at the first tick.
    pub(crate) fn new() -> Self {
        Self {
            dirty: true,
            ..Self::default()
        }
    }

    /// A thread stream was loaded: its header may hold a snooze.
    pub(crate) fn loaded(&mut self) {
        self.dirty = true;
    }

    /// A commit returned holding `event`.
    pub(crate) fn committed(&mut self, event: &clauseo_protocol::DomainEvent) {
        if self.recheck
            || matches!(event, clauseo_protocol::DomainEvent::ThreadSnoozed(_))
            || clauseo_reducer::wakes_thread(event)
        {
            self.dirty = true;
        }
    }
}

/// After every input: fires an armed timer whose wall time has passed, else re-arms when the
/// earliest snooze may have moved. Nothing while a wake runs or the host is stopping.
pub(crate) fn tick(actor: &mut Actor) {
    if matches!(actor.phase, HostPhase::Stopping(_)) {
        take_timer(actor);
        return;
    }
    if actor.triage.busy {
        return;
    }
    if let Some((_, at)) = &actor.triage.timer
        && *at <= actor.now().wall
    {
        take_timer(actor);
        wake(actor);
        return;
    }
    if std::mem::take(&mut actor.triage.dirty) {
        rearm(actor);
    }
}

/// The timer armed with `generation` fired; a stale fire does nothing.
pub(crate) fn on_wake(actor: &mut Actor, generation: u64) {
    if generation != actor.triage.generation || actor.triage.busy {
        return;
    }
    take_timer(actor);
    wake(actor);
}

/// A wake's commit returned: arm for the next snooze.
pub(crate) fn wake_committed(actor: &mut Actor, outcome: Result<(), &Failure>) {
    if let Err(failure) = outcome
        && failure.first
    {
        tracing::warn!(
            error = %crate::log_file::Cause(&failure.error),
            "a snooze's wake was not stored"
        );
    }
    actor.triage.busy = false;
    actor.triage.dirty = true;
}

/// The earliest `snoozed_until` of a thread that can be written now; notes whether a snoozed
/// thread was passed over for a commit in flight.
fn earliest(actor: &mut Actor) -> Option<Timestamp> {
    let mut skipped = false;
    let mut next: Option<Timestamp> = None;
    for &thread in actor.threads.keys() {
        let Some(facts) = facts(actor, thread) else {
            continue;
        };
        let Some(until) = facts.header.snoozed_until else {
            continue;
        };
        if facts.writing {
            skipped = true;
            continue;
        }
        next = Some(next.map_or(until, |next| next.min(until)));
    }
    actor.triage.recheck = skipped;
    next
}

/// Arms the timer for the earliest snooze, replacing the armed one only when that time moved,
/// or cancels it when no thread is snoozed.
fn rearm(actor: &mut Actor) {
    let next = earliest(actor);
    let armed = actor.triage.timer.as_ref().map(|(_, at)| *at);
    if next == armed {
        return;
    }
    take_timer(actor);
    let Some(at) = next else {
        return;
    };
    let now = actor.now().wall;
    let millis = at.as_millis().saturating_sub(now.as_millis()).max(0);
    let delay = Duration::from_millis(u64::try_from(millis).unwrap_or(0));
    let generation = actor.triage.generation;
    let handle = actor.post_after(delay, Input::SnoozeWake { generation });
    actor.triage.timer = Some((handle, at));
}

/// Takes the armed timer, if any, and moves the generation on.
fn take_timer(actor: &mut Actor) {
    if let Some((timer, _)) = actor.triage.timer.take() {
        timer.abort();
    }
    actor.triage.generation = actor.triage.generation.wrapping_add(1);
}

/// The threads that can be written now whose snooze has ended at `now`.
fn ended(actor: &Actor, now: Timestamp) -> Vec<ThreadId> {
    actor
        .threads
        .keys()
        .copied()
        .filter(|&thread| {
            facts(actor, thread).is_some_and(|facts| {
                !facts.writing && facts.header.snoozed_until.is_some_and(|until| until <= now)
            })
        })
        .collect()
}

/// Wakes every thread whose snooze has ended: loads them, checks each again on its loaded
/// state, and logs their wakes in one commit.
fn wake(actor: &mut Actor) {
    let threads = ended(actor, actor.now().wall);
    if threads.is_empty() {
        actor.triage.dirty = true;
        return;
    }
    actor.triage.busy = true;
    load_all(
        actor,
        &threads,
        Box::new(|actor: &mut Actor, loaded: &[ThreadId]| {
            let now = actor.now().wall;
            let mut draft = Draft::new();
            for thread in ended(actor, now) {
                if !loaded.contains(&thread) {
                    continue;
                }
                let event = ThreadSnoozed {
                    until: None,
                    by: SnoozeBy::Expired,
                };
                let stream = StreamId::Thread(thread);
                let pushed = actor.pipeline.push(
                    &mut draft,
                    stream,
                    event.into(),
                    None,
                    now,
                    &mut actor.ids,
                );
                if let Err(error) = pushed {
                    tracing::warn!(
                        %thread,
                        error = %crate::log_file::Cause(&error),
                        "a snooze's wake was not logged"
                    );
                }
            }
            let sent = if draft.is_empty() {
                None
            } else {
                Some(actor.send_commit(draft, Tag::Wake))
            };
            match sent {
                Some(Ok(_)) => {}
                // Nothing to commit after all (a load or a push failed, or a thread changed
                // meanwhile): no re-arm until a commit or a load says something moved, so a
                // thread that cannot be woken does not spin the timer.
                Some(Err(error)) => {
                    tracing::warn!(
                        error = %crate::log_file::Cause(&error),
                        "a snooze's wake was not sent"
                    );
                    actor.triage.busy = false;
                }
                None => actor.triage.busy = false,
            }
        }),
    );
}
