//! Each thread's denials for the turn they came in: the intake's rate cap, the counts toward
//! the pause, the tool calls watched for an access denied, and one card per kind and target.
//! Pure bookkeeping: the caller gives the time and mints the ids.
//!
//! - **Rate cap:** at most [`DENIALS_PER_WINDOW`] denials per [`DENIAL_WINDOW`] pass the intake
//!   per thread (the sinks' `CappedSink` caps each launch the same way upstream; this one also
//!   covers the tool-call source). Those past it are counted toward the pause, never recorded.
//! - **Pause:** [`PAUSE_IN_A_ROW`] counted denials in a row, or [`PAUSE_IN_TURN`] in the turn,
//!   pause it once. Counted: what lies beyond the matter (a path `Outside`, `Hidden` or
//!   `LinkTarget`, a site); a `Dropped` adds its count. "In a row" starts again when a tool call
//!   ends `completed` with no counted denial since the previous tool call ended.
//! - **Dedupe:** one card per kind and target per turn; a repeat grows its count.

use std::collections::BTreeMap;
use std::time::Duration;

use clauseo_protocol::{DenialId, DenialKind, ThreadId, ToolCallId, TurnId};
pub(crate) use clauseo_sandbox::{DENIAL_WINDOW, DENIALS_PER_WINDOW};

use crate::error::Error;

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

/// Every thread's denials, on the actor.
#[derive(Debug, Default)]
pub(crate) struct Denials {
    threads: BTreeMap<ThreadId, TurnDenials>,
}

/// One thread's denials, all in `turn`.
#[derive(Debug)]
struct TurnDenials {
    turn: Option<TurnId>,
    /// The rate cap's window: when it began and how many passed in it.
    window: Option<(Duration, u32)>,
    in_a_row: u32,
    in_turn: u32,
    /// A counted denial came since the last tool call ended.
    since_tool_end: bool,
    paused: bool,
    cards: Vec<(DenialKind, DenialId, u32)>,
    /// The tool calls running, with the paths each named.
    calls: BTreeMap<ToolCallId, Vec<String>>,
    capped_logged: bool,
}

impl TurnDenials {
    fn new(turn: Option<TurnId>) -> Self {
        Self {
            turn,
            window: None,
            in_a_row: 0,
            in_turn: 0,
            since_tool_end: false,
            paused: false,
            cards: Vec::new(),
            calls: BTreeMap::new(),
            capped_logged: false,
        }
    }
}

/// What the intake makes of one denial.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Admitted {
    /// It passed the rate cap: check and record it.
    pub(crate) pass: bool,
    /// It paused the turn: log `DenialsPaused` with this count and cancel, once.
    pub(crate) pause: Option<(TurnId, u16)>,
}

/// What recording a checked denial comes to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Recorded {
    /// Log `DenialRecorded` with this id and count (1 for a new card).
    Log { denial: DenialId, count: u32 },
    /// The turn holds [`MAX_CARDS_PER_TURN`] cards: not logged; `first` for the first.
    Capped { first: bool },
}

impl Denials {
    /// The thread's state for `turn`, fresh when the turn changed.
    fn turn(&mut self, thread: ThreadId, turn: Option<TurnId>) -> &mut TurnDenials {
        let state = self
            .threads
            .entry(thread)
            .or_insert_with(|| TurnDenials::new(turn));
        if state.turn != turn {
            *state = TurnDenials::new(turn);
        }
        state
    }

    /// One denial at `now` in `turn` (`None` when no turn runs), counting `counted` toward the
    /// pause (0, 1, or a `Dropped`'s count). `rated`: whether it takes a place under the rate
    /// cap (a `Dropped` summary does not).
    pub(crate) fn admit(
        &mut self,
        thread: ThreadId,
        turn: Option<TurnId>,
        now: Duration,
        counted: u32,
        rated: bool,
    ) -> Admitted {
        let state = self.turn(thread, turn);
        let pass = rated && {
            let (start, passed) = match state.window {
                Some((start, passed)) if now.saturating_sub(start) < DENIAL_WINDOW => {
                    (start, passed)
                }
                _ => (now, 0),
            };
            let pass = passed < DENIALS_PER_WINDOW;
            state.window = Some((start, passed.saturating_add(u32::from(pass))));
            pass
        };
        if counted > 0 {
            state.in_a_row = state.in_a_row.saturating_add(counted);
            state.in_turn = state.in_turn.saturating_add(counted);
            state.since_tool_end = true;
        }
        let due = state.in_a_row >= PAUSE_IN_A_ROW || state.in_turn >= PAUSE_IN_TURN;
        let pause = match turn {
            Some(turn) if due && !state.paused => {
                state.paused = true;
                Some((turn, u16::try_from(state.in_turn).unwrap_or(u16::MAX)))
            }
            _ => None,
        };
        Admitted { pass, pause }
    }

    /// A tool call of `turn` ended: `completed` with no counted denial since the last end
    /// starts "in a row" again; any end closes the span.
    pub(crate) fn tool_ended(&mut self, thread: ThreadId, turn: TurnId, completed: bool) {
        let state = self.turn(thread, Some(turn));
        if completed && !state.since_tool_end {
            state.in_a_row = 0;
        }
        state.since_tool_end = false;
    }

    /// Records a checked denial of `kind`, which came in `turn`, while `running` is the
    /// thread's turn now. A repeat of a card of the running turn grows its count; a new one gets
    /// `mint`'s id. A denial of a turn that has ended since gets a card of its own, not kept.
    pub(crate) fn record(
        &mut self,
        thread: ThreadId,
        turn: Option<TurnId>,
        running: Option<TurnId>,
        kind: &DenialKind,
        mint: impl FnOnce() -> Result<DenialId, Error>,
    ) -> Result<Recorded, Error> {
        if turn != running {
            return Ok(Recorded::Log {
                denial: mint()?,
                count: 1,
            });
        }
        let state = self.turn(thread, turn);
        if let Some((_, denial, count)) = state.cards.iter_mut().find(|(k, ..)| k == kind) {
            *count = count.saturating_add(1);
            return Ok(Recorded::Log {
                denial: *denial,
                count: *count,
            });
        }
        if state.cards.len() >= MAX_CARDS_PER_TURN {
            let first = !state.capped_logged;
            state.capped_logged = true;
            return Ok(Recorded::Capped { first });
        }
        let denial = mint()?;
        state.cards.push((kind.clone(), denial, 1));
        Ok(Recorded::Log { denial, count: 1 })
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
            if kept.len() < MAX_PATHS_PER_CALL && !kept.contains(&path) {
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

    /// The cards of `thread`'s turn and their counts (tests).
    #[cfg(test)]
    pub(crate) fn cards(&self, thread: ThreadId) -> Vec<(DenialId, u32)> {
        self.threads.get(&thread).map_or_else(Vec::new, |state| {
            state.cards.iter().map(|(_, id, n)| (*id, *n)).collect()
        })
    }

    /// Forgets each thread's denials whose turn `running` no longer gives for it.
    pub(crate) fn forget_ended(&mut self, running: impl Fn(ThreadId) -> Option<TurnId>) {
        self.threads
            .retain(|thread, state| running(*thread) == state.turn);
    }
}
