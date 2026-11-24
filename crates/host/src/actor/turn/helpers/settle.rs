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
/// report keeps the message's start up to the first chunk that would pass it, and every word
/// is in the helper's `HelperText` all the same.
pub(super) const LAST_WORDS_CAP_BYTES: usize = 1024 * 1024;

/// One open helper's last words: the text of its latest message since its latest step.
#[derive(Debug, Default)]
pub(super) struct LastWords {
    /// The message's ACP id, when its chunks carry one.
    message: Option<CompactString>,
    text: String,
    /// A chunk of this message passed the cap: no later chunk is kept, so the text stays a
    /// clean start of the message.
    cut: bool,
}

/// Keeps `text`, words of the open helper `helper`, as its last words: added to its message,
/// or in place of what was kept when the chunk starts another message. From the first chunk
/// that would pass [`LAST_WORDS_CAP_BYTES`] on, the message's chunks are not kept.
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
        last.cut = false;
    }
    if last.cut {
        return;
    }
    if last.text.len() + text.len() <= LAST_WORDS_CAP_BYTES {
        last.text.push_str(text);
    } else {
        last.cut = true;
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

#[cfg(test)]
mod tests {
    use clauseo_protocol::{CompactString, HelperId};

    use super::{Helpers, LAST_WORDS_CAP_BYTES, note_words};
    use crate::pipeline::tests::support::id;

    fn kept(helpers: &Helpers, helper: HelperId) -> Option<&str> {
        helpers
            .last_words
            .get(&helper)
            .map(|last| last.text.as_str())
    }

    /// The first chunk that would pass the cap ends what is kept of the message: a smaller chunk
    /// after it is not added, so the report has no gap; the next message starts afresh.
    #[test]
    fn last_words_stop_at_the_first_chunk_past_the_cap() {
        let mut helpers = Helpers::default();
        let helper: HelperId = id(1);
        let message = CompactString::from("m-1");
        let start = "a".repeat(LAST_WORDS_CAP_BYTES - 10);
        note_words(&mut helpers, helper, Some(&message), &start);
        note_words(&mut helpers, helper, Some(&message), &"b".repeat(20));
        note_words(&mut helpers, helper, Some(&message), "c");
        assert_eq!(kept(&helpers, helper), Some(start.as_str()));
        let next = CompactString::from("m-2");
        note_words(&mut helpers, helper, Some(&next), "Done.");
        assert_eq!(kept(&helpers, helper), Some("Done."));
    }
}
