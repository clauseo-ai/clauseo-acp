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
//! Ends: the agent's own (Claude's handback, or a foreground launch step's final status), here;
//! the ends a cut-short run brings (a Stop, a turn's close, a detach, a failed write's retry, a
//! past run's helpers left open in the log) in `ends.rs`.

use std::collections::{BTreeMap, HashMap};

use clauseo_acp::{AcpEvent, AgentUpdate, MessageRole, Origin};
use clauseo_protocol::{
    AgentSessionId, CompactString, ConfigCategory, ConfigOption, ConfigOptionKind, ContentBlock,
    DomainEvent, HelperId, HelperOutcome, HelperStarted, HelperText, INLINE_LIMIT_BYTES, StreamId,
    TextOrBlob, ThreadId, ToolCallId, TurnId,
};

use super::super::Actor;
use super::out::Out;
use super::{Then, TimerKind, TurnTimer, lost, state, updates};

use crate::build;
use crate::coalesce::DURABLE_BATCH;
use crate::error::Error;
use crate::log_file::Cause;
use crate::pipeline::Draft;

pub(super) mod claude;
mod ends;
mod words;

#[cfg(test)]
mod tests;

pub(crate) use ends::settle_loaded;
pub(in crate::actor::turn) use ends::{on_cancel, on_close, on_detach, reopen_from_view};

/// The helpers of one conversation, from its first helper on.
#[derive(Debug, Default)]
pub(super) struct Helpers {
    /// Every helper started, open or ended, by the agent's id for it. A `BTreeMap`, so walking
    /// it has one order.
    known: BTreeMap<CompactString, Known>,
    /// Launch steps whose helper has not started yet, by the step's ACP id (Claude: an `Agent`
    /// call whose input has not come).
    launches: HashMap<CompactString, claude::Launch>,
    /// Steps stamped with a helper and not logged yet, by the step's ACP id.
    steps: HashMap<CompactString, HelperId>,
    /// Claude's handback calls, by their ACP id, until their update says whether it landed. A
    /// `BTreeMap`, so walking it has one order.
    handbacks: BTreeMap<CompactString, claude::Handback>,
    /// Words not logged yet, in first-word order.
    words: words::Held,
    /// The helper's own ends ([`end`]) this run sent and the log may not hold yet: the outcome
    /// and report of each, so a retry after a failed write writes that same end (`ends.rs`).
    /// Each stays through retries whose writes fail too, and goes once the view after a drained
    /// stream shows it landed, once a turn's end is durable (every commit before it is then), or
    /// with the helpers at a detach.
    sent_ends: BTreeMap<HelperId, (HelperOutcome, Option<String>)>,
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

    /// Clauseo's id for the helper the agent knows as `helper`: a started one's, or the one a
    /// launch whose input has not come already took.
    fn id_of(&self, helper: &str) -> Option<HelperId> {
        self.known
            .get(helper)
            .map(|known| known.id)
            .or_else(|| self.launches.get(helper).and_then(claude::Launch::id))
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
    let model = start.model.map(|model| {
        let options = actor
            .loaded(StreamId::Thread(thread))
            .and_then(|loaded| loaded.state.as_thread())
            .and_then(|state| state.header())
            .map_or(&[][..], |header| &header.config_options[..]);
        listed_model(model, options)
    });
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
        model,
    });
    updates::commit(actor, thread, [built.value.into()], built.blobs)
}

/// The model id the conversation's own model menu lists for `model`: `model` itself when the
/// menu has it; else the first id of the menu's `Model` option that holds `model` as one of
/// its `-`-separated words (Claude Code names a helper's model by its alias, `haiku`, and
/// the menu lists `claude-haiku-5-5`); else `model` as it came.
pub(super) fn listed_model(model: CompactString, options: &[ConfigOption]) -> CompactString {
    let choices = options
        .iter()
        .filter(|option| option.category == ConfigCategory::Model)
        .filter_map(|option| {
            if let ConfigOptionKind::Select { options, .. } = &option.kind {
                Some(options)
            } else {
                None
            }
        })
        .flatten();
    let mut family = None;
    for choice in choices {
        if choice.value == model {
            return model;
        }
        let word = choice
            .value
            .split('-')
            .any(|word| word.eq_ignore_ascii_case(&model));
        if word && family.is_none() {
            family = Some(choice.value.clone());
        }
    }
    family.unwrap_or(model)
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
    match helpers.id_of(helper) {
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
/// durable batch's timer at the latest). A helper whose launch input has not come yet takes
/// them under the id its launch already took, as its steps do. Words of a helper the host does
/// not know are dropped.
pub(super) fn words(
    actor: &mut Actor,
    thread: ThreadId,
    turn: TurnId,
    helper: &str,
    text: &str,
) -> Result<(), Error> {
    let Some(helpers) = existing(actor, thread) else {
        tracing::debug!(%thread, %helper, "words of a helper not started were dropped");
        return Ok(());
    };
    let Some(id) = helpers.id_of(helper) else {
        tracing::debug!(%thread, %helper, "words of a helper not started were dropped");
        return Ok(());
    };
    let pushed = helpers.words.push(id, turn, text);
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
                StreamId::Thread(thread),
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
    let id = known.id;
    helpers.sent_ends.insert(id, (outcome, summary.clone()));
    let built = build::helper_ended(id, outcome, summary.map(TextOrBlob::Inline));
    updates::commit(actor, thread, [built.value.into()], built.blobs)
}

/// The thread's turn end is durable, and every commit before it: no end [`end`] sent is lost.
pub(super) fn ends_durable(actor: &mut Actor, thread: ThreadId) {
    if let Some(helpers) = existing(actor, thread) {
        helpers.sent_ends.clear();
    }
}

/// An update the agent stamped with the helper it knows as `helper`. Replayed and late ones
/// are dropped. A new step goes through the lead's step path with the helper set (Claude's
/// handback is its end instead, `claude.rs`); a step update goes as the lead's (it names its
/// step by id); answer text becomes the helper's words (held for the agent's own turn when none
/// is live, as steps are); everything else (thoughts, plans, usage, mode, options, titles) is
/// dropped, so a helper never touches the conversation's own.
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
            acp_message_id,
            content: ContentBlock::Text { text },
        } => match updates::live_turn(actor, thread, origin) {
            Some(turn) => words(actor, thread, turn, helper, &text),
            // With no turn live the words wait for the agent's own turn, as its steps do.
            None if origin == Origin::Idle => {
                let update = AgentUpdate::MessageChunk {
                    role: MessageRole::Answer,
                    acp_message_id,
                    content: ContentBlock::Text { text },
                };
                let event = AcpEvent::HelperUpdate {
                    session: session.clone(),
                    origin,
                    helper: helper.clone(),
                    update,
                };
                super::own_turn::hold_helper(actor, thread, event);
                Ok(())
            }
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
