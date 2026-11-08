//! Each thread's denials: the intake's rate cap, the checks in flight, the counts toward the
//! pause, the tool calls watched for an access denied, and one card per kind and target in a
//! turn. Pure bookkeeping: the caller gives the time and mints the ids.
//!
//! - **Rate cap:** a sliding window per thread: at most [`DENIALS_PER_WINDOW`] counted denials
//!   (and the tool-call source) in any [`DENIAL_WINDOW`], and as many again of the uncounted
//!   ones (`ReadOnly`, an upstream sign-in), so these never use up the window of the others.
//!   Those past it are counted toward the pause, never recorded.
//! - **In flight:** at most [`MAX_IN_FLIGHT`] path checks run off the actor per thread, each
//!   held until its thread really ends (a hung lookup keeps its place); one past them is dropped
//!   and counted toward the pause, as a `Dropped` is.
//! - **Pause:** [`PAUSE_IN_A_ROW`] counted denials in a row, or [`PAUSE_IN_TURN`] in the turn,
//!   pause it once. Counted: what lies beyond the matter (a path `Outside`, `Hidden` or
//!   `LinkTarget`, a site) at the intake; a tool-call denial when it is recorded, and not when an
//!   OS report of the same target counted already (one refusal, two reports, is counted once);
//!   a `Dropped` adds its count. "In a row" starts again when a tool call seen running ends
//!   `completed` with no counted denial since the previous tool call ended.
//! - **Dedupe:** one card per kind and target per turn, the target by identity when it has one,
//!   else by its text folded on a folding volume; a repeat grows its count. A card not offered
//!   that an offered decision of the same target follows is replaced by a new, offered one. A
//!   denial checked after its turn ended goes to that turn's cards (kept until the next turn
//!   ends), with their dedupe and cap; one older than that is not recorded.

mod turn;

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use clauseo_protocol::{DenialId, DenialKind, ThreadId, ToolCallId, TurnId};
pub(crate) use clauseo_sandbox::{DENIAL_WINDOW, DENIALS_PER_WINDOW};

use super::check::SessionCache;
use super::offer::identity::Target;
use crate::error::Error;
use turn::TurnDenials;

#[cfg(test)]
mod tests;

/// Counted denials in a row that pause the turn.
pub(crate) const PAUSE_IN_A_ROW: u32 = 3;
/// Counted denials in one turn that pause it.
pub(crate) const PAUSE_IN_TURN: u32 = 20;
/// The most cards one turn records; a new kind or target past them is not recorded.
pub(crate) const MAX_CARDS_PER_TURN: usize = 64;
/// The most tool calls one turn watches for an access denied; a new call past them is not.
pub(crate) const MAX_WATCHED_CALLS: usize = 256;
/// The most paths one watched tool call keeps.
pub(crate) const MAX_PATHS_PER_CALL: usize = 4;
/// The longest path a watched tool call keeps, in bytes; a longer one is not kept.
pub(crate) const MAX_PATH_BYTES: usize = 4096;
/// The most path checks one thread runs off the actor at once.
pub(crate) const MAX_IN_FLIGHT: usize = 2;

/// Every thread's denials, on the actor.
#[derive(Debug, Default)]
pub(crate) struct Denials {
    threads: BTreeMap<ThreadId, ThreadDenials>,
}

/// One thread's denials.
#[derive(Debug, Default)]
struct ThreadDenials {
    /// The running turn's (or, with none, `None`'s).
    now: TurnDenials,
    /// The last turn's that ended, for its late checks.
    ended: Option<TurnDenials>,
    /// The rate cap's windows: the counted denials and the tool-call source, then the others.
    counted: Window,
    uncounted: Window,
    /// The checks running off the actor, each place given back when its thread ends.
    in_flight: Arc<AtomicUsize>,
    /// The session's capture of its rules and places, shared with its checks.
    session: SessionCache,
}

/// A sliding window: when each denial that passed in the last [`DENIAL_WINDOW`] came.
#[derive(Debug, Default)]
struct Window {
    passed: VecDeque<Duration>,
}

impl Window {
    /// Whether one more at `now` passes, and if so it takes a place.
    fn pass(&mut self, now: Duration) -> bool {
        while self
            .passed
            .front()
            .is_some_and(|at| now.saturating_sub(*at) >= DENIAL_WINDOW)
        {
            self.passed.pop_front();
        }
        let cap = usize::try_from(DENIALS_PER_WINDOW).unwrap_or(usize::MAX);
        let pass = self.passed.len() < cap;
        if pass {
            self.passed.push_back(now);
        }
        pass
    }
}

/// Which window a denial takes a place in, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Rate {
    /// Counted denials and the tool-call source.
    Counted,
    /// `ReadOnly` and an upstream sign-in.
    Uncounted,
    /// A `Dropped` summary: no place, never recorded.
    Unrated,
}

/// What the intake makes of one denial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Admitted {
    /// It passed the rate cap: check and record it.
    pub(crate) pass: bool,
    /// It paused the turn: log `DenialsPaused` with this count and cancel, once.
    pub(crate) pause: Option<(TurnId, u16)>,
}

/// A place among a thread's checks in flight, given back when dropped: moved into the check's
/// work, so a check whose thread hangs keeps it.
#[derive(Debug)]
pub(crate) struct InFlight(Arc<AtomicUsize>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// One checked denial to record.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Record<'a> {
    /// The card.
    pub(crate) kind: &'a DenialKind,
    /// Whether it may widen it.
    pub(crate) offered: bool,
    /// What the card is about, for the dedupe.
    pub(crate) card: &'a Target,
    /// The path refused (resolved and pinned), for the pause's one count per refusal and a
    /// tool call's confirmation; `None` for a site.
    pub(crate) asked: Option<&'a Target>,
    /// From the OS (the sandbox, the proxy), not a tool call.
    pub(crate) os: bool,
}

/// What recording a checked denial comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Recorded {
    /// Log `DenialRecorded` with this id and count (1 for a new card).
    Log { denial: DenialId, count: u32 },
    /// The turn holds [`MAX_CARDS_PER_TURN`] cards: not logged; `first` for the first.
    Capped { first: bool },
    /// Its turn ended before the last one: not recorded.
    Stale,
}

impl Denials {
    fn thread(&mut self, thread: ThreadId) -> &mut ThreadDenials {
        self.threads.entry(thread).or_default()
    }

    /// The thread's state for `turn`, fresh when the turn changed (the ended one kept).
    fn turn(&mut self, thread: ThreadId, turn: Option<TurnId>) -> &mut TurnDenials {
        let state = self.thread(thread);
        if state.now.turn != turn {
            let ended = std::mem::replace(&mut state.now, TurnDenials::new(turn));
            if ended.turn.is_some() {
                state.ended = Some(ended);
            }
        }
        &mut state.now
    }

    /// One denial at `now` in `turn` (`None` when no turn runs), counting `counted` toward the
    /// pause (0, 1, or a `Dropped`'s count), taking a place in `rate`'s window.
    pub(crate) fn admit(
        &mut self,
        thread: ThreadId,
        turn: Option<TurnId>,
        now: Duration,
        counted: u32,
        rate: Rate,
    ) -> Admitted {
        let state = self.thread(thread);
        let pass = match rate {
            Rate::Counted => state.counted.pass(now),
            Rate::Uncounted => state.uncounted.pass(now),
            Rate::Unrated => false,
        };
        let pause = self.turn(thread, turn).count(counted);
        Admitted { pass, pause }
    }

    /// The thread's session capture, for a check to use or fill.
    pub(crate) fn session(&mut self, thread: ThreadId) -> SessionCache {
        self.thread(thread).session.clone()
    }

    /// A place for one more check off the actor for `thread`, or `None` when
    /// [`MAX_IN_FLIGHT`] already run.
    pub(crate) fn start_check(&mut self, thread: ThreadId) -> Option<InFlight> {
        let flight = &self.thread(thread).in_flight;
        if flight.load(Ordering::SeqCst) >= MAX_IN_FLIGHT {
            return None;
        }
        // Only the actor adds, so the check above holds.
        flight.fetch_add(1, Ordering::SeqCst);
        Some(InFlight(Arc::clone(flight)))
    }

    /// A check dropped for want of a place: counted toward the pause (one), as a `Dropped`.
    pub(crate) fn dropped_check(
        &mut self,
        thread: ThreadId,
        turn: Option<TurnId>,
    ) -> Option<(TurnId, u16)> {
        self.turn(thread, turn).count(1)
    }

    /// A tool call of `turn` ended: `completed` with no counted denial since the last end
    /// starts "in a row" again; any end closes the span. The caller tells only of a call it saw
    /// running ([`Denials::watching`]).
    pub(crate) fn tool_ended(&mut self, thread: ThreadId, turn: TurnId, completed: bool) {
        let state = self.turn(thread, Some(turn));
        if completed && !state.since_tool_end {
            state.in_a_row = 0;
        }
        state.since_tool_end = false;
    }

    /// Whether an OS report of the same file as `asked`, by identity, came in `turn`.
    pub(crate) fn confirmed(&self, thread: ThreadId, turn: Option<TurnId>, asked: &Target) -> bool {
        self.threads.get(&thread).is_some_and(|state| {
            [Some(&state.now), state.ended.as_ref()]
                .into_iter()
                .flatten()
                .filter(|held| held.turn == turn)
                .any(|held| held.confirmed(asked))
        })
    }

    /// Records a checked denial, which came in `turn`, while `running` is the thread's turn
    /// now; and a pause, when a tool call's count makes one. A repeat of a card grows its
    /// count; a new one gets `mint`'s id.
    pub(crate) fn record(
        &mut self,
        thread: ThreadId,
        turn: Option<TurnId>,
        running: Option<TurnId>,
        record: Record<'_>,
        mint: impl FnOnce() -> Result<DenialId, Error>,
    ) -> Result<(Recorded, Option<(TurnId, u16)>), Error> {
        if turn == running {
            let state = self.turn(thread, turn);
            let pause = state.count_once(&record);
            return Ok((state.card(&record, mint)?, pause));
        }
        let state = self.thread(thread);
        match state.ended.as_mut().filter(|ended| ended.turn == turn) {
            Some(ended) => Ok((ended.card(&record, mint)?, None)),
            None => Ok((Recorded::Stale, None)),
        }
    }

    /// Whether `call` of `turn` is watched: seen running, not yet ended.
    pub(crate) fn watching(&self, thread: ThreadId, turn: TurnId, call: ToolCallId) -> bool {
        self.threads.get(&thread).is_some_and(|state| {
            state.now.turn == Some(turn) && state.now.calls.contains_key(&call)
        })
    }

    /// Adds `paths` to the tool call `call` of `turn`, watched until it ends.
    pub(crate) fn watch(
        &mut self,
        thread: ThreadId,
        turn: TurnId,
        call: ToolCallId,
        paths: impl IntoIterator<Item = String>,
    ) {
        let state = self.turn(thread, Some(turn));
        if !state.calls.contains_key(&call) && state.calls.len() >= MAX_WATCHED_CALLS {
            return;
        }
        let kept = state.calls.entry(call).or_default();
        for path in paths {
            if kept.len() < MAX_PATHS_PER_CALL
                && path.len() <= MAX_PATH_BYTES
                && !kept.contains(&path)
            {
                kept.push(path);
            }
        }
    }

    /// The paths the tool call `call` of `turn` named, no longer watched.
    pub(crate) fn unwatch(
        &mut self,
        thread: ThreadId,
        turn: TurnId,
        call: ToolCallId,
    ) -> Vec<String> {
        self.turn(thread, Some(turn))
            .calls
            .remove(&call)
            .unwrap_or_default()
    }

    /// The target of `denial`'s card, if the thread's last two turns hold it: what R18 3b pins
    /// a grant to ([`Target::same_file`]).
    #[cfg_attr(
        not(test),
        expect(dead_code, reason = "R18 3b's grant pins to it; nothing grants in 3a")
    )]
    pub(crate) fn card_target(&self, thread: ThreadId, denial: DenialId) -> Option<&Target> {
        let state = self.threads.get(&thread)?;
        [Some(&state.now), state.ended.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|held| held.target_of(denial))
    }

    /// The cards of `thread`'s running turn and their counts (tests).
    #[cfg(test)]
    pub(crate) fn cards(&self, thread: ThreadId) -> Vec<(DenialId, u32)> {
        self.threads
            .get(&thread)
            .map_or_else(Vec::new, |state| state.now.counts())
    }

    /// The checks of `thread` in flight (tests).
    #[cfg(test)]
    pub(crate) fn in_flight(&self, thread: ThreadId) -> usize {
        self.threads
            .get(&thread)
            .map_or(0, |state| state.in_flight.load(Ordering::SeqCst))
    }

    /// Forgets each thread `alive` no longer gives, once none of its checks is in flight.
    pub(crate) fn prune(&mut self, alive: impl Fn(ThreadId) -> bool) {
        self.threads
            .retain(|thread, state| alive(*thread) || state.in_flight.load(Ordering::SeqCst) > 0);
    }
}
