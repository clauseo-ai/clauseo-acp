//! The constructors of a turn's in-memory state: a new current turn, and the ways a turn ends;
//! whether start-up or shutdown waits for the turn path; whether an update is the live turn's;
//! and what a thread's turn state holds.

use std::collections::HashSet;
use std::time::Duration;

use clauseo_protocol::{
    ContentBlock, DetachReason, ErrorCode, ExpiryReason, MatterId, MessageId, StopReason, TextKind,
    ThreadId, TurnId, ViewEntry,
};
use clauseo_reducer::ThreadState;

use super::super::{Actor, HostPhase};
use super::{Current, End, TurnPhase, host_stopping, key_retry};

/// The detail of a `TurnFailed` logged because a commit of the turn failed.
pub(super) const STORE_DETAIL: &str = "a commit of the turn failed";

impl End {
    /// A turn the lawyer (or shutdown) cancelled.
    pub(super) fn cancelled(detach: Option<DetachReason>) -> Self {
        Self {
            stop: StopReason::Cancelled,
            failure: None,
            expiry: ExpiryReason::TurnEnded,
            detach,
        }
    }

    /// A turn that failed with `code`.
    pub(super) fn failed(
        code: ErrorCode,
        detail: Option<String>,
        detach: Option<DetachReason>,
    ) -> Self {
        let expiry = if code == ErrorCode::AgentExited {
            ExpiryReason::AgentExited
        } else {
            ExpiryReason::TurnEnded
        };
        Self {
            stop: StopReason::Failed,
            failure: Some((code, detail)),
            expiry,
            detach,
        }
    }

    /// A commit of the turn failed with `code`: the turn ends `Failed` with it, unless it
    /// already ends with a failure of its own (the first failure is the one logged).
    pub(super) fn store_failed(&mut self, code: Option<ErrorCode>) {
        if let Some(code) = code
            && self.failure.is_none()
        {
            self.stop = StopReason::Failed;
            self.failure = Some((code, Some(STORE_DETAIL.to_owned())));
        }
    }
}

/// Where the prompt's task leaves the listing of the agent's descendants; `None` until it is
/// taken, and when it failed.
pub(super) type BeforePrompt = std::rc::Rc<std::cell::RefCell<Option<clauseo_process::Snapshot>>>;

impl Current {
    pub(super) fn new(
        turn: TurnId,
        message: MessageId,
        prompt: Vec<ContentBlock>,
        started: std::time::Duration,
    ) -> Self {
        Self {
            turn,
            message,
            prompt,
            phase: TurnPhase::Accepting,
            cancelled: false,
            stopped: key_retry::Stopped::default(),
            refused: None,
            limit: None,
            tool_paths: crate::sent::TurnPaths::default(),
            checkpoint_before: None,
            prompt_call: None,
            case_law_hint: crate::connectors::hint::TurnLooks::Nothing,
            cancel_timer: None,
            before_prompt: None,
            end: None,
            failure: None,
            offline: None,
            abort: false,
            store_failed: None,
            detach: None,
            limit_read: None,
            started,
            first_words: None,
            first_update: None,
            withheld: super::withheld::Withheld::default(),
        }
    }

    /// Whether agent updates of the turn are logged: its prompt is in flight, not cancelled.
    pub(super) fn live(&self) -> bool {
        self.phase == TurnPhase::Prompting && !self.cancelled
    }
}

/// Whether start-up (reconciliation) or shutdown (cancels, agent stops, unlogged stops at the
/// limit) must wait for the turn path. Scanned only while starting or stopping.
pub(crate) fn busy(actor: &Actor) -> bool {
    if matches!(actor.phase, HostPhase::Serving) {
        return false;
    }
    let stops = host_stopping(actor) && super::super::limits::stops_busy(actor);
    stops
        || actor.threads.values().any(|entry| {
            let state = &entry.turn;
            state.reconciling
                || state.agent.is_some()
                || state.preparing.is_some()
                || state.current.as_ref().is_some_and(|current| {
                    matches!(
                        current.phase,
                        TurnPhase::Checkpointing
                            | TurnPhase::Starting
                            | TurnPhase::Prompting
                            | TurnPhase::FallbackStop
                            | TurnPhase::Ending
                    )
                })
        })
}

/// Whether an update of `turn` belongs to `thread`'s live turn (its prompt in flight, not
/// cancelled). The first one has its time kept and a lifecycle line marks it (D-022).
pub(super) fn live(actor: &mut Actor, thread: ThreadId, turn: TurnId) -> bool {
    let now = actor.now().mono;
    let Some(current) =
        super::current(actor, thread).filter(|current| current.turn == turn && current.live())
    else {
        return false;
    };
    if current.first_update.is_none() {
        current.first_update = Some(now);
        tracing::info!(target: crate::log_file::LIFECYCLE, %thread, %turn, "first update");
    }
    true
}

/// Whether `thread` has a current turn, a last turn whose end is not durable yet, or queued
/// turns (`thread.turn_running` for `ArchiveThread`).
pub(crate) fn turn_running(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        let state = &entry.turn;
        state.current.is_some() || state.closing.is_some() || !state.queue.is_empty()
    })
}

/// How long after the first answered turn ends an agent's title is still taken. The adapters
/// make their AI title in the background after `end_turn`, so it can land a while later. The
/// next prompt closes the window sooner (`prompt_sent`).
pub(crate) const TITLE_GRACE: Duration = Duration::from_secs(60);

/// Where a thread's name stands for the agent's titles (D-070), in this run of the host. It
/// opens for the thread's first answered turn: `Grace` from that turn's end, then `Settled`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TitleWindow {
    /// No answered turn has ended in this run, and the history loaded or unloaded so far has
    /// none (`history_settles`).
    #[default]
    Open,
    /// The first answered turn ended at this monotonic time: titles are taken until
    /// `TITLE_GRACE` has passed or the next prompt goes out.
    Grace(Duration),
    /// The name is settled: no agent title is taken.
    Settled,
}

/// The first answered turn of `thread` ended at `now`: its window opens with the grace.
pub(super) fn answered_end(actor: &mut Actor, thread: ThreadId, now: Duration) {
    if let Some(entry) = actor.threads.get_mut(&thread)
        && entry.turn.title == TitleWindow::Open
    {
        entry.turn.title = TitleWindow::Grace(now);
    }
}

/// The folded history of `thread`, as its stream loads or unloads: an answered turn that ended
/// settles an open window, so a restart's titles and the titles after a unload between turns
/// read the history the store holds (D-070). A grace window is left to run out.
pub(crate) fn history_settles(actor: &mut Actor, thread: ThreadId, stream: &ThreadState) {
    if let Some(entry) = actor.threads.get_mut(&thread)
        && entry.turn.title == TitleWindow::Open
        && answered_turn_ended(stream)
    {
        entry.turn.title = TitleWindow::Settled;
    }
}

/// A turn's prompt went out: the next question closes a grace window for good.
pub(super) fn prompt_sent(actor: &mut Actor, thread: ThreadId) {
    if let Some(entry) = actor.threads.get_mut(&thread)
        && let TitleWindow::Grace(_) = entry.turn.title
    {
        entry.turn.title = TitleWindow::Settled;
    }
}

/// Whether an agent title of `thread` may be taken now (D-070). `idle` says the update came
/// outside the live turn. The origin turns idle once a prompt's answer is in, before the host
/// handles the reply, so an idle update during a prompt that has answered counts as after the
/// answer: the window opens now.
pub(super) fn title_open(actor: &mut Actor, thread: ThreadId, idle: bool) -> bool {
    let answered = idle
        && actor
            .threads
            .get(&thread)
            .and_then(|entry| entry.turn.current.as_deref())
            .is_some_and(|current| {
                current.phase == TurnPhase::Prompting && current.first_words.is_some()
            });
    if answered {
        let now = actor.now().mono;
        answered_end(actor, thread, now);
    }
    let now = actor.now().mono;
    let Some(entry) = actor.threads.get(&thread) else {
        return false;
    };
    match entry.turn.title {
        TitleWindow::Open => true,
        TitleWindow::Grace(at) => now.saturating_sub(at) <= TITLE_GRACE,
        TitleWindow::Settled => false,
    }
}

/// Whether the folded thread has a step of a turn that also has non-empty answer text.
fn answered_turn_ended(stream: &ThreadState) -> bool {
    let answered: HashSet<TurnId> = stream
        .entries()
        .values()
        .filter_map(|entry| {
            if let ViewEntry::AgentText(text) = entry
                && text.kind == TextKind::Answer
                && !text.text.is_empty()
            {
                Some(text.turn)
            } else {
                None
            }
        })
        .collect();
    stream
        .entries()
        .values()
        .any(|entry| matches!(entry, ViewEntry::Step(step) if answered.contains(&step.turn)))
}

/// The turn running on `thread` now, if any (`blocked.rs` attributes a blocked site to it).
pub(crate) fn running_turn(actor: &Actor, thread: ThreadId) -> Option<TurnId> {
    let current = actor.threads.get(&thread)?.turn.current.as_deref()?;
    Some(current.turn)
}

/// Whether `thread`'s stream stays loaded: an agent, open turn, repair, retriable end, limit stop.
pub(crate) fn keeps_loaded(actor: &Actor, thread: ThreadId) -> bool {
    actor.threads.get(&thread).is_some_and(|entry| {
        let state = &entry.turn;
        state.agent.is_some()
            || state.preparing.is_some()
            || state.current.is_some()
            || state.reconciling
            || state
                .closing
                .as_ref()
                .is_some_and(|closing| !closing.retried)
    }) || super::super::limits::holds_stop(actor, thread)
}

/// Whether `matter`'s stream, once loaded to check a reused checkpoint, stays loaded: while a
/// turn, an undo or a restore holds its writer, so the turn's next checkpoint reads it without
/// a reload.
pub(crate) fn keeps_matter_loaded(actor: &Actor, matter: MatterId) -> bool {
    actor
        .matters
        .get(&matter)
        .is_some_and(|entry| entry.turns.holder.is_some())
}
