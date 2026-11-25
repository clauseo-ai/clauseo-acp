//! Codex's helpers, as codex-acp 2.10.0 reports them on a connection with
//! `ConnectOptions::helper_sessions` (the host turns it on for Codex only, `agents/launch.rs`).
//!
//! - `AcpEvent::HelperSpawned` makes a helper with no `call` (codex-acp swallows the lead's spawn
//!   call), its `parent` from the helpers this conversation knows, its `name`, and no `task`
//!   (Codex encrypts it). Its `HelperStarted` waits ([`State::pending`]) until its model is
//!   known: every helper's first update is a `config_option_update` whose model option names
//!   the model it runs on (`crates/acp`'s fixture `codex-acp-2.10.0-helpers-x1.jsonl`). Any
//!   other first update, a report, an end, a helper of its own, an ask of its own, or the
//!   close of its turn starts it without a model.
//! - Its steps, step updates and words come as `AcpEvent::HelperUpdate` (`helpers.rs`,
//!   [`before_update`] first); its config, usage, status and title updates are dropped there.
//! - `AcpEvent::HelperAnswer` (its `final_answer` chunks) is its report: kept, not shown as
//!   words, and sent as the end's `summary`, cut at 1 MiB as Claude's last words are
//!   ([`Report`]). The host relies on acp handing the report over before the end
//!   (`crates/acp`'s helper tests pin that order): a piece after the end is dropped.
//! - `AcpEvent::HelperEnded`: `completed` ends it `Done`, `failed` `Failed`, `cancelled` and
//!   `disconnected` `Stopped`, each with the report so far. acp passes on no other state. Every
//!   other end of a card (a Stop or any cancel, a failed turn, the agent's exit) carries the report
//!   so far too (`ends.rs`): a stopped helper keeps what it had written of its answer.
//!   A replayed end (after `session/load`, codex-acp replays each helper as `disconnected`)
//!   changes nothing.
//! - What comes `Late` (after a cancel; acp also tags an ended helper's frames so) is dropped,
//!   as every other late helper frame is (`helpers::on_update`): the cancel's commit has ended
//!   every open card already, so codex-acp's own `cancelled` after a Stop adds no second end.
//!   A late spawn makes no card either, but marks the agent to stop ([`on_spawned`]).
//!
//! A Codex helper is a background helper (`Known::background`): it runs on its own session and
//! has its own end, so a turn that ends by itself leaves it open (`ends.rs`), and its
//! `HelperEnded` ends it whenever it comes. Every cancel, an engine death and a load end it
//! `Stopped`, as they end every helper.
//!
//! codex-acp never stops a helper on a cancel (recorded: it interrupts only the lead, and the
//! helper ran on until the adapter's process ended): `helpers::on_cancel` marks the agent
//! to stop once the turn has closed, and its next prompt starts it again and loads the session
//! (`idle::Relaunch::HelpersRunning`).

use std::collections::{BTreeMap, BTreeSet};

use clauseo_acp::{AgentUpdate, HelperState, Origin};
use clauseo_protocol::{
    AgentSessionId, CompactString, ConfigCategory, ConfigOptionKind, ContentBlock, HelperOutcome,
    ThreadId, TurnId,
};

use super::super::super::Actor;
use super::super::out::Out;
use super::super::updates;
use super::settle::{keep_capped, with_cut_mark};
use super::{Start, existing, helpers_mut, known};
use crate::error::Error;
use crate::log_file::Cause;

/// Codex's part of a conversation's helpers.
#[derive(Debug, Default)]
pub(in super::super) struct State {
    /// Helpers spawned whose `HelperStarted` waits for their model, by the helper's session id.
    pending: BTreeMap<CompactString, Pending>,
    /// Each open helper's report so far, by the helper's session id.
    answers: BTreeMap<CompactString, Report>,
    /// Step ids a helper sent that the conversation already had as another step: the helper's
    /// step and its updates are dropped, so the earlier step is left as it was.
    clashed: BTreeSet<CompactString>,
}

impl State {
    /// Takes the report kept for helper `helper`, which ends now, as its end's summary: cut at
    /// the cap with the mark ([`Report`]); `None` when it wrote none.
    pub(super) fn take_report(&mut self, helper: &str) -> Option<String> {
        self.answers
            .remove(helper)
            .filter(|report| !report.text.is_empty())
            .map(Report::into_summary)
    }
}

/// A helper's report as kept: at most `LAST_WORDS_CAP_BYTES` (1 MiB), the cap Claude's
/// helpers' last words have (`settle.rs`). From the first piece that would pass it, nothing more
/// is kept, so the report stays a clean start (a first piece that alone passes it keeps its
/// start up to the cap); every word of it is the helper's own all the same. A cut report ends
/// with a line break and the cut mark.
#[derive(Debug, Default)]
struct Report {
    text: String,
    /// A piece passed the cap.
    cut: bool,
}

impl Report {
    /// Keeps `piece` unless the report was cut or `piece` would take it past the cap; a first
    /// piece that alone passes the cap keeps its start up to it (`settle::keep_capped`).
    fn keep(&mut self, piece: &str) {
        if self.cut {
            return;
        }
        self.cut = keep_capped(&mut self.text, "", piece);
    }

    /// The report as the end's summary.
    fn into_summary(self) -> String {
        with_cut_mark(self.text, self.cut)
    }
}

/// A helper spawned whose start waits for its model.
#[derive(Debug)]
struct Pending {
    /// The turn it was spawned in.
    turn: TurnId,
    /// What it starts with.
    start: Start,
}

/// codex-acp spawned helper `helper` (its session id), from `parent` (another helper's session
/// id) or the lead. Only a live turn's spawn counts: a replayed one (`session/load`) or a late
/// one makes no card. A late one is a helper that started while a Stop landed (the spawn was in
/// flight at the cancel): codex-acp does not stop it, and nothing says it will send its end, so
/// the agent is marked to stop once the cancelled turn has closed, as when a cancel finds a helper
/// open (`idle::stop_for_helpers`); its next prompt loads the session. Its start waits for its
/// model ([`before_update`]).
pub(in super::super) fn on_spawned(
    actor: &mut Actor,
    thread: ThreadId,
    origin: Origin,
    helper: CompactString,
    parent: Option<CompactString>,
    name: CompactString,
) {
    match origin {
        Origin::Replay => return,
        Origin::Late(_) => {
            tracing::info!(%thread, %helper, "a helper spawned as a Stop landed; no card");
            super::super::idle::stop_for_helpers(actor, thread);
            return;
        }
        Origin::Turn(_) | Origin::Idle => {}
    }
    let Some(turn) = updates::live_turn(actor, thread, origin) else {
        tracing::info!(%thread, %helper, "a helper spawned outside a turn was dropped");
        return;
    };
    let parent = parent.and_then(|parent| {
        release(actor, thread, &parent);
        let id = known(actor, thread, &parent).map(|known| known.id);
        if id.is_none() {
            tracing::info!(%thread, %parent, "a helper's parent was never started");
        }
        id
    });
    let Some(helpers) = helpers_mut(actor, thread) else {
        return;
    };
    if helpers.known.contains_key(&helper) || helpers.codex.pending.contains_key(&helper) {
        tracing::debug!(%thread, %helper, "a helper spawned twice");
        return;
    }
    let start = Start {
        id: None,
        acp_id: helper.clone(),
        parent,
        call: None,
        name,
        label: None,
        task: None,
        model: None,
        background: true,
        codex: true,
    };
    helpers
        .codex
        .pending
        .insert(helper, Pending { turn, start });
}

/// Logs the start of helper `helper` if it waits for its model, with `model` (`None`: not
/// known). Nothing for a helper that does not wait.
fn release_with(actor: &mut Actor, thread: ThreadId, helper: &str, model: Option<CompactString>) {
    let Some(Pending { turn, mut start }) =
        existing(actor, thread).and_then(|helpers| helpers.codex.pending.remove(helper))
    else {
        return;
    };
    start.model = model;
    if let Err(error) = super::start(actor, thread, turn, start) {
        tracing::warn!(%thread, error = %Cause(&error), "a helper's start was not logged");
    }
}

/// Logs the start of helper `helper` without a model if it still waits for one.
pub(in super::super) fn release(actor: &mut Actor, thread: ThreadId, helper: &str) {
    release_with(actor, thread, helper, None);
}

/// Runs before a helper's update is handled (`helpers::on_update`), and gives it back unless it
/// was used up here. A helper that waits for its model starts: with the model its options
/// name, which uses the update up, or without one, before any other update. A Codex helper's
/// step whose id the conversation already has, and that step's updates, are dropped with a log
/// line: they never change another step.
pub(super) fn before_update(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    helper: &str,
    update: AgentUpdate,
) -> Option<AgentUpdate> {
    let waits =
        existing(actor, thread).is_some_and(|helpers| helpers.codex.pending.contains_key(helper));
    if waits {
        if let AgentUpdate::ConfigOptions(options) = &update {
            let model = options
                .iter()
                .filter(|option| option.category == ConfigCategory::Model)
                .find_map(|option| match &option.kind {
                    ConfigOptionKind::Select { current_value, .. } => Some(current_value.clone()),
                    ConfigOptionKind::Boolean { .. } | ConfigOptionKind::Unknown { .. } => None,
                });
            if model.is_some() {
                release_with(actor, thread, helper, model);
                return None;
            }
        }
        release(actor, thread, helper);
    }
    if !known(actor, thread, helper).is_some_and(|known| known.codex) {
        return Some(update);
    }
    match &update {
        AgentUpdate::ToolCall(call) => {
            if updates::known_tool(actor, thread, session, &call.acp_id).is_some() {
                tracing::warn!(
                    %thread,
                    %helper,
                    acp_id = %call.acp_id,
                    "a helper's step has the id of a step already shown; it and its updates are \
                     dropped"
                );
                if let Some(helpers) = existing(actor, thread) {
                    helpers.codex.clashed.insert(call.acp_id.clone());
                }
                return None;
            }
        }
        AgentUpdate::ToolCallUpdate { acp_id, .. } => {
            let clashed = existing(actor, thread)
                .is_some_and(|helpers| helpers.codex.clashed.contains(acp_id));
            if clashed {
                return None;
            }
        }
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
        | AgentUpdate::CompactionSummaryChunk { .. } => {}
    }
    Some(update)
}

/// A piece of helper `helper`'s report (its `final_answer` chunks): kept for its end's
/// `summary`. Replayed and late pieces, pieces of a helper never spawned or already ended, and
/// blocks other than text are dropped.
pub(in super::super) fn on_answer(
    actor: &mut Actor,
    thread: ThreadId,
    origin: Origin,
    helper: &CompactString,
    content: ContentBlock,
) {
    if matches!(origin, Origin::Replay | Origin::Late(_)) {
        return;
    }
    release(actor, thread, helper);
    let open = known(actor, thread, helper).is_some_and(|known| known.open);
    let ContentBlock::Text { text } = content else {
        tracing::debug!(%thread, %helper, "a helper's report block that is not text was dropped");
        return;
    };
    if !open {
        tracing::debug!(%thread, %helper, "a report of a helper not open was dropped");
        return;
    }
    if let Some(helpers) = existing(actor, thread) {
        helpers
            .codex
            .answers
            .entry(helper.clone())
            .or_default()
            .keep(&text);
    }
}

/// codex-acp says helper `helper` ended in `state`. A replayed or late end changes nothing; any
/// other ends the helper once (`helpers::end`), with the report kept so far, whatever the
/// outcome.
pub(in super::super) fn on_ended(
    actor: &mut Actor,
    thread: ThreadId,
    origin: Origin,
    helper: &CompactString,
    state: HelperState,
) {
    if matches!(origin, Origin::Replay | Origin::Late(_)) {
        return;
    }
    release(actor, thread, helper);
    let outcome = match state {
        HelperState::Completed => HelperOutcome::Done,
        HelperState::Failed => HelperOutcome::Failed,
        HelperState::Cancelled | HelperState::Disconnected => HelperOutcome::Stopped,
    };
    let summary = existing(actor, thread).and_then(|helpers| helpers.codex.take_report(helper));
    if let Err(error) = super::end(actor, thread, helper, outcome, summary) {
        tracing::warn!(%thread, error = %Cause(&error), "a helper's end was not logged");
    }
}

/// Whether the thread has a Codex helper spawned that has not ended: the one case where a
/// cancel leaves work running in the agent's process.
pub(super) fn any_open(helpers: &super::Helpers) -> bool {
    !helpers.codex.pending.is_empty()
        || helpers
            .known
            .values()
            .any(|known| known.open && known.codex)
}

/// Before open helpers of the thread end at once (a Stop, a turn's close, the session's end):
/// each helper whose start still waits for its model starts in `out`, without one. Those a
/// turn that ended by itself leaves open stay open, started.
pub(super) fn start_waiting(
    actor: &mut Actor,
    thread: ThreadId,
    out: &mut Out,
) -> Result<(), Error> {
    let Some(helpers) = existing(actor, thread) else {
        return Ok(());
    };
    // By helper id: Codex's helper ids are UUID v7, so this is spawn order.
    let waiting = std::mem::take(&mut helpers.codex.pending);
    for pending in waiting.into_values() {
        if let Some(built) = super::prepare(actor, thread, pending.turn, pending.start)? {
            out.push(actor, built.value)?;
            out.draft.blobs(built.blobs);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::settle::LAST_WORDS_CAP_BYTES;
    use super::Report;

    /// A report keeps its pieces up to the cap; the first piece past it, and every piece after,
    /// is not kept, and the summary ends with a line break and the cut mark. One under the cap
    /// has no mark.
    #[test]
    fn a_codex_report_is_cut_at_the_cap_with_the_mark() {
        let start = "a".repeat(LAST_WORDS_CAP_BYTES - 10);
        let mut report = Report::default();
        report.keep(&start);
        report.keep(&"b".repeat(20));
        report.keep("c");
        assert_eq!(report.into_summary(), format!("{start}\n[…]"));
        let mut whole = Report::default();
        whole.keep(&start);
        whole.keep(&"b".repeat(10));
        assert_eq!(whole.into_summary().len(), LAST_WORDS_CAP_BYTES);
    }

    /// A first piece that alone passes the cap, with a two-byte "é" straddling it: the report
    /// keeps the piece's start, at most the cap and cut back to a character boundary, then a
    /// line break and the mark; a later piece is not kept.
    #[test]
    fn a_first_piece_past_the_cap_is_kept_up_to_the_cap_with_the_mark() {
        let start = "a".repeat(LAST_WORDS_CAP_BYTES - 1);
        let mut report = Report::default();
        report.keep(&format!("{start}é and more"));
        report.keep("later");
        let summary = report.into_summary();
        let text = summary.strip_suffix("\n[…]").unwrap();
        assert!(text.len() <= LAST_WORDS_CAP_BYTES, "{}", text.len());
        assert_eq!(text, start);
    }
}
