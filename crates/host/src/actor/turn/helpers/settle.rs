//! A helper's end as the engine says it: Claude Code's task frames, which claude-agent-acp
//! passes on and `clauseo_acp` reads into [`AcpEvent::HelperSettled`] (the launch id and the
//! outcome, nothing else). Under Claude Code's code mode a helper offered only `REPL` has no
//! handback tool, so this is the only end it sends.
//!
//! - Either end counts and the first wins: a handback ends the card, and a settle after it
//!   changes nothing; a settle ends the card, and a handback after it changes nothing (`end`
//!   ends a helper once). An ended helper stays ended.
//! - A settle naming no helper the host started (a helper's own shell command runs as a task
//!   too, its settle naming the `Bash` step) changes nothing.
//! - A settle that comes while a handback call of the helper waits for its update ends nothing:
//!   that update ends the helper, by the handback's own rule (its report, `success: false`
//!   ends nothing). If it never comes, the turn's end, a Stop, a detach or a load ends it.
//! - The report of a helper that ends `Done` on a settle: the helper's last words the host holds
//!   ([`LastWords`]). Nothing of the frames' own text reaches the host.
//! - It ends the helper in whatever turn is live, the agent's own included, or with none live,
//!   as a handback does. With output waiting for the agent's own turn (`own_turn.rs`) it waits
//!   behind it, so the helper's words held there come first and make its report.
//! - A settle that comes after a cancel (`Origin::Late`) is dropped: the cancel ends every
//!   helper `Stopped`.

use clauseo_acp::{AcpEvent, Origin};
use clauseo_protocol::{AgentSessionId, CompactString, HelperId, HelperOutcome, ThreadId};

use super::super::super::Actor;
use super::super::own_turn;
use super::{Helpers, end, existing};
use crate::log_file::Cause;

/// The most of one helper's last words kept for its report: the largest text chunk
/// `clauseo_acp` lets through (1 MiB). A model's message stays far below it; past it the
/// report keeps the message's start, and every word is in the helper's `HelperText` all the
/// same.
pub(super) const LAST_WORDS_CAP_BYTES: usize = 1024 * 1024;

/// One open helper's last words: the text of its latest message since its latest step.
#[derive(Debug, Default)]
pub(super) struct LastWords {
    /// The message's ACP id, when its chunks carry one.
    message: Option<CompactString>,
    text: String,
}

/// Keeps `text`, words of the open helper `helper`, as its last words: added to its message,
/// or in place of what was kept when the chunk starts another message.
pub(super) fn note_words(
    helpers: &mut Helpers,
    helper: HelperId,
    message: Option<&CompactString>,
    text: &str,
) {
    let last = helpers.last_words.entry(helper).or_default();
    if last.message.as_ref() != message {
        last.message = message.cloned();
        last.text.clear();
    }
    if last.text.len() + text.len() <= LAST_WORDS_CAP_BYTES {
        last.text.push_str(text);
    }
}

/// The engine says the task of the helper the agent knows as `helper` settled with `outcome`.
pub(in crate::actor::turn) fn on_settled(
    actor: &mut Actor,
    thread: ThreadId,
    session: &AgentSessionId,
    origin: Origin,
    helper: &CompactString,
    outcome: HelperOutcome,
) {
    match origin {
        Origin::Late(_) | Origin::Replay => return,
        Origin::Idle if own_turn::waits(actor, thread) => {
            let event = AcpEvent::HelperSettled {
                session: session.clone(),
                origin,
                helper: helper.clone(),
                outcome,
            };
            own_turn::hold_helper(actor, thread, event);
            return;
        }
        Origin::Idle | Origin::Turn(_) => {}
    }
    let Some(helpers) = existing(actor, thread) else {
        tracing::debug!(%thread, %helper, "a settle of no helper");
        return;
    };
    let Some(known) = helpers.known.get(helper.as_str()).copied() else {
        tracing::debug!(%thread, %helper, "a settle of no helper");
        return;
    };
    if !known.open {
        return;
    }
    // Its handback's update ends it, as with no settle.
    let handing_back = helpers
        .handbacks
        .values()
        .any(|handback| handback.helper() == helper);
    if handing_back {
        tracing::debug!(%thread, %helper, "a settle while a handback waits for its update");
        return;
    }
    let report = if outcome == HelperOutcome::Done {
        last_words(helpers, known.id)
    } else {
        None
    };
    if let Err(error) = end(actor, thread, helper, outcome, report) {
        tracing::warn!(%thread, error = %Cause(&error), "a helper's settle was not logged");
    }
}

/// The report of the helper Clauseo knows as `id` as it ends `Done` on a settle: its last
/// words; `None` without any.
fn last_words(helpers: &Helpers, id: HelperId) -> Option<String> {
    helpers
        .last_words
        .get(&id)
        .filter(|last| !last.text.is_empty())
        .map(|last| last.text.clone())
}
