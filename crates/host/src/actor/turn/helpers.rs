//! Helpers (subagents the lead agent launches) of one conversation: their start, the steps and
//! words stamped with them, and their end, logged as `HelperStarted`, `ToolCallStarted` with
//! `helper` set, `HelperText` and `HelperEnded`. Every agent feeds the same interface:
//! [`start`], [`stamp_step`], [`words`] and [`end`]; how Claude says each is in `claude.rs`.
//!
//! The state lives on the thread ([`Helpers`], boxed in `TurnState::helpers`), keyed by the
//! agent's id for each helper, never by position. It exists from the conversation's first
//! helper on: a conversation without one pays a `None` check per tool call update and a
//! tool-name compare per tool call, and nothing per text chunk.
//!
//! Ends: the agent's own (Claude's handback, or a foreground launch step's final status); a Stop
//! ends every open helper `Stopped`; a turn that ends by itself with helpers still open ends them
//! `Failed` (they never handed back), unless a send-now cut that turn short; the agent session
//! ending ends them `Stopped`.

use std::collections::HashMap;

use clauseo_acp::{AgentUpdate, MessageRole, Origin};
use clauseo_protocol::{
    AgentSessionId, CompactString, ContentBlock, DomainEvent, HelperId, HelperOutcome,
    HelperStarted, HelperText, INLINE_LIMIT_BYTES, StopReason, TextOrBlob, ThreadId, ToolCallId,
    TurnId,
};

use super::super::Actor;
use super::out::Out;
use super::{Then, TimerKind, TurnPhase, TurnTimer, current, lost, state, updates};
use crate::build;
use crate::coalesce::DURABLE_BATCH;
use crate::error::Error;
use crate::log_file::Cause;
use crate::pipeline::Draft;

pub(super) mod claude;
mod words;

/// The helpers of one conversation, from its first helper on.
#[derive(Debug, Default)]
pub(super) struct Helpers {
    /// Every helper started, open or ended, by the agent's id for it.
    known: HashMap<CompactString, Known>,
    /// Launch steps whose helper has not started yet, by the step's ACP id (Claude: an `Agent`
    /// call whose input has not come).
    launches: HashMap<CompactString, claude::Launch>,
    /// Steps stamped with a helper and not logged yet, by the step's ACP id.
    steps: HashMap<CompactString, HelperId>,
    /// Claude's handback calls, by their ACP id, until their update says whether it landed.
    handbacks: HashMap<CompactString, claude::Handback>,
    /// Words not logged yet, in first-word order.
    words: words::Held,
    /// The turn a send-now cut short: its end leaves the helpers open.
    kept: Option<TurnId>,
}

/// A helper the host started.
#[derive(Debug, Clone, Copy)]
pub(super) struct Known {
    /// Clauseo's id for it.
    pub(super) id: HelperId,
    /// It has not ended.
    pub(super) open: bool,
    /// Its launch step returned at once and it works on (Claude's background helper): the
    /// step's end is not the helper's.
    pub(super) background: bool,
}

/// What a helper starts with, from the agent.
#[derive(Debug, Clone)]
pub(super) struct Start {
    /// Clauseo's id for it, when its launch step already took one; else minted at the start.
    pub(super) id: Option<HelperId>,
    /// The agent's id for it.
    pub(super) acp_id: CompactString,
    /// The helper that launched it; `None` for the lead.
    pub(super) parent: Option<HelperId>,
    /// The step that launched it.
    pub(super) call: Option<ToolCallId>,
    /// Its name.
    pub(super) name: CompactString,
    /// A short label for its task.
    pub(super) label: Option<CompactString>,
    /// What it was asked.
    pub(super) task: Option<String>,
    /// The model it runs on, when known.
    pub(super) model: Option<CompactString>,
    /// Its launch step already said it works on in the background.
    pub(super) background: bool,
}

impl Helpers {
    /// The helper of the step ACP knows as `acp_id`, taken as the step is logged.
    pub(super) fn step_of(&mut self, acp_id: &str) -> Option<HelperId> {
        self.steps.remove(acp_id)
    }

    /// Whether words are held for the next commit.
    pub(super) fn has_words(&self) -> bool {
        !self.words.is_empty()
    }
}

/// The thread's helpers, made on first use.
fn helpers_mut(actor: &mut Actor, thread: ThreadId) -> Option<&mut Helpers> {
    Some(
        state(actor, thread)?
            .helpers
            .get_or_insert_with(Box::default),
    )
}

/// The thread's helpers, if it has had one.
fn existing(actor: &mut Actor, thread: ThreadId) -> Option<&mut Helpers> {
    state(actor, thread)?.helpers.as_deref_mut()
}

/// Starts a helper: logs `HelperStarted` and keeps it open. A second start of the same agent id
/// changes nothing.
pub(super) fn start(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    start: Start,
) -> Result<(), Error> {
    let helpers = helpers_mut(actor, thread).ok_or_else(|| lost("a thread"))?;
    if helpers.known.contains_key(&start.acp_id) {
        return Ok(());
    }
    let id = match start.id {
        Some(id) => id,
        None => actor.ids.mint(actor.clock.now().wall)?,
    };
    let helpers = helpers_mut(actor, thread).ok_or_else(|| lost("a thread"))?;
    helpers.known.insert(
        start.acp_id.clone(),
        Known {
            id,
            open: true,
            background: start.background,
        },
    );
    let built = build::helper_started(HelperStarted {
        turn,
        helper: id,
        acp_id: start.acp_id,
        parent: start.parent,
        call: start.call,
        name: start.name,
        label: start.label,
        task: start.task.map(TextOrBlob::Inline),
        model: start.model,
    });
    updates::commit(actor, thread, [built.value.into()], built.blobs)
}

/// Keeps the helper a step stamped with `helper` belongs to until the step is logged
/// ([`Helpers::step_of`] takes it back). A stamp the host does not know logs one line, and the
/// step shows as the lead's. An ended helper keeps its steps: a late step still shows under its
/// card.
pub(super) fn stamp_step(actor: &mut Actor, thread: ThreadId, helper: &str, step: &CompactString) {
    let Some(helpers) = existing(actor, thread) else {
        tracing::info!(%thread, %helper, "a step of a helper not started; shown as the lead's");
        return;
    };
    // A launch whose input has not come already has its helper's id.
    let id = helpers
        .known
        .get(helper)
        .map(|known| known.id)
        .or_else(|| helpers.launches.get(helper).and_then(claude::Launch::id));
    match id {
        Some(id) => {
            helpers.steps.insert(step.clone(), id);
        }
        None => {
            tracing::info!(%thread, %helper, "a step of a helper not started; shown as the lead's");
        }
    }
}

/// The helper the agent knows as `helper`, if started.
pub(super) fn known(actor: &mut Actor, thread: ThreadId, helper: &str) -> Option<Known> {
    existing(actor, thread)?.known.get(helper).copied()
}

/// Marks the helper the agent knows as `helper` as working on after its launch step returns.
pub(super) fn set_background(actor: &mut Actor, thread: ThreadId, helper: &str) {
    let Some(helpers) = existing(actor, thread) else {
        tracing::debug!(%thread, %helper, "a background mark for no helper");
        return;
    };
    if let Some(known) = helpers.known.get_mut(helper) {
        known.background = true;
    } else if let Some(launch) = helpers.launches.get_mut(helper) {
        launch.background = true;
    } else {
        tracing::debug!(%thread, %helper, "a background mark for no helper");
    }
}

/// Holds `text`, the helper's own words in `turn`, for the next commit of the thread (the
/// durable batch's timer at the latest). Words of a helper the host does not know are dropped.
pub(super) fn words(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    helper: &str,
    text: &str,
) -> Result<(), Error> {
    let Some(known) = known(actor, thread, helper) else {
        tracing::debug!(%thread, %helper, "words of a helper not started were dropped");
        return Ok(());
    };
    let helpers = existing(actor, thread).ok_or_else(|| lost("the helpers"))?;
    let pushed = helpers.words.push(known.id, turn, text);
    if pushed.first {
        let timer = TurnTimer {
            thread,
            kind: TimerKind::Durable,
        };
        let _handle = actor.arm_timer(DURABLE_BATCH, timer);
    }
    if pushed.full {
        Out::new(actor, thread, None)?.send(actor, Then::Nothing)?;
    }
    Ok(())
}

/// Whether the thread holds helper words not logged yet.
pub(super) fn has_words(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .and_then(|entry| entry.turn.helpers.as_deref())
        .is_some_and(Helpers::has_words)
}

/// Moves the thread's held helper words into `draft` as `HelperText` events, in first-word
/// order, each at most `INLINE_LIMIT_BYTES` after escaping. Every commit of the thread runs it,
/// after the lead's text (`out.rs`).
pub(super) fn take_words(
    actor: &mut Actor,
    thread: ThreadId,
    draft: &mut Draft,
) -> Result<(), Error> {
    let Some(held) = existing(actor, thread).map(|helpers| helpers.words.take()) else {
        return Ok(());
    };
    let ts = actor.now().wall;
    for run in held {
        for text in words::cut(run.text, INLINE_LIMIT_BYTES) {
            let event = HelperText {
                turn: run.turn,
                helper: run.helper,
                text,
            };
            actor.pipeline.push(
                draft,
                clauseo_protocol::StreamId::Thread(thread),
                DomainEvent::HelperText(event),
                None,
                ts,
                &mut actor.ids,
            )?;
        }
    }
    Ok(())
}

/// Ends the helper the agent knows as `helper` with `outcome`, once: its held words go first.
/// An unknown or ended helper changes nothing.
pub(super) fn end(
    actor: &mut Actor,
    thread: ThreadId,
    helper: &str,
    outcome: HelperOutcome,
    summary: Option<String>,
) -> Result<(), Error> {
    let Some(helpers) = existing(actor, thread) else {
        return Ok(());
    };
    let Some(known) = helpers.known.get_mut(helper).filter(|known| known.open) else {
        tracing::debug!(%thread, %helper, "an end of a helper not open");
        return Ok(());
    };
    known.open = false;
    let built = build::helper_ended(known.id, outcome, summary.map(TextOrBlob::Inline));
    updates::commit(actor, thread, [built.value.into()], built.blobs)
}

/// Pushes `HelperEnded { outcome }` for every open helper of the thread into `out`, and closes
/// them.
fn end_open(
    actor: &mut Actor,
    thread: ThreadId,
    outcome: HelperOutcome,
    out: &mut Out,
) -> Result<(), Error> {
    let Some(helpers) = existing(actor, thread) else {
        return Ok(());
    };
    let mut ended: Vec<HelperId> = Vec::new();
    for known in helpers.known.values_mut().filter(|known| known.open) {
        known.open = false;
        ended.push(known.id);
    }
    // In start order, so the log reads as the helpers ran.
    ended.sort_unstable();
    for helper in ended {
        out.push(actor, build::helper_ended(helper, outcome, None).value)?;
    }
    Ok(())
}

/// A cancel of `turn` (the lawyer's Stop, a failure, the host stopping): every open helper ends
/// `Stopped`, unless `send_now` (the lawyer's next message cut the turn short), which leaves
/// them open and keeps them open at the turn's end.
pub(super) fn on_cancel(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    send_now: bool,
    out: &mut Out,
) -> Result<(), Error> {
    if send_now {
        if let Some(helpers) = existing(actor, thread) {
            helpers.kept = Some(turn);
        }
        return Ok(());
    }
    end_open(actor, thread, HelperOutcome::Stopped, out)
}

/// The close of `turn` (`close.rs`): with the agent gone (`detached`) every open helper ends
/// `Stopped`; a cancelled turn's left `Stopped` too; a turn that ended by itself leaves them
/// `Failed`, since none handed back a report. A turn a send-now cut short, and a turn whose
/// prompt never went out, leave them open.
pub(super) fn on_close(
    actor: &mut Actor,
    out: &mut Out,
    turn: TurnId,
    stop: StopReason,
    detached: bool,
) -> Result<(), Error> {
    let thread = out.thread();
    // Only a turn whose prompt went out closes helpers: a queued turn removed, or one that
    // failed before its prompt, leaves them as they are.
    let prompted = current(actor, thread).is_some_and(|current| {
        current.turn == turn
            && matches!(
                current.phase,
                TurnPhase::Prompting | TurnPhase::FallbackStop | TurnPhase::Ending
            )
    });
    let Some(helpers) = existing(actor, thread) else {
        return Ok(());
    };
    let kept = helpers.kept.take_if(|kept| *kept == turn).is_some();
    let outcome = if detached {
        HelperOutcome::Stopped
    } else if kept || !prompted {
        return Ok(());
    } else if stop == StopReason::Cancelled {
        HelperOutcome::Stopped
    } else {
        HelperOutcome::Failed
    };
    end_open(actor, thread, outcome, out)
}

/// The agent session ended outside a turn: every open helper ends `Stopped`, and the thread
/// forgets its helpers, whose ids belonged to that session.
pub(super) fn on_detach(actor: &mut Actor, thread: ThreadId, out: &mut Out) -> Result<(), Error> {
    end_open(actor, thread, HelperOutcome::Stopped, out)?;
    if let Some(state) = state(actor, thread) {
        state.helpers = None;
    }
    Ok(())
}

/// An update the agent stamped with the helper it knows as `helper`. Replayed and late ones
/// are dropped. A new step goes through the lead's step path with the helper set (Claude's
/// handback is its end instead, `claude.rs`); a step update goes as the lead's (it names its
/// step by id); answer text becomes the helper's words; everything else (thoughts, plans,
/// usage, mode, options, titles) is dropped, so a helper never touches the conversation's own.
pub(super) fn on_update(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    origin: Origin,
    helper: &CompactString,
    update: AgentUpdate,
) {
    if matches!(origin, Origin::Replay | Origin::Late(_)) {
        return;
    }
    let done = match update {
        AgentUpdate::ToolCall(call) => {
            if claude::is_handback(call.tool_name.as_deref()) {
                claude::handback_started(actor, thread, helper, &call);
                return;
            }
            stamp_step(actor, thread, helper, &call.acp_id);
            updates::on_update(actor, thread, session, origin, AgentUpdate::ToolCall(call));
            Ok(())
        }
        update @ AgentUpdate::ToolCallUpdate { .. } => {
            updates::on_update(actor, thread, session, origin, update);
            Ok(())
        }
        AgentUpdate::MessageChunk {
            role: MessageRole::Answer,
            content: ContentBlock::Text { text },
            ..
        } => match updates::live_turn(actor, thread, origin) {
            Some(turn) => words(actor, thread, turn, helper, &text),
            None => {
                tracing::debug!(%thread, "a helper's words outside a turn were dropped");
                Ok(())
            }
        },
        AgentUpdate::MessageChunk { .. }
        | AgentUpdate::Plan(_)
        | AgentUpdate::Commands(_)
        | AgentUpdate::CurrentMode { .. }
        | AgentUpdate::ConfigOptions(_)
        | AgentUpdate::SessionTitle { .. }
        | AgentUpdate::Usage { .. }
        | AgentUpdate::Notice { .. }
        | AgentUpdate::CompactionStarted { .. }
        | AgentUpdate::CompactionEnded { .. }
        | AgentUpdate::CompactionSummaryChunk { .. } => Ok(()),
    };
    if let Err(error) = done {
        tracing::warn!(%thread, error = %Cause(&error), "a helper's update was not logged");
    }
}
