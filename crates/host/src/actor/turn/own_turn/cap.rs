//! The cap on agent output held for a turn (`Held::updates`). While a close or an undo keeps
//! the agent's own turn from opening, a background helper's words pile up there chunk by chunk;
//! past [`CAP_BYTES`] the oldest words go (the lead's or a helper's text chunks), down to
//! [`KEEP_BYTES`], with one warning per pile-up. Only words go: every other event is kept, since
//! each one changes what the turn does (a step a card points at, a helper's launch, handback or
//! settle, the costed usage that ends the agent's own turn, a permission request or an
//! elicitation the agent waits on), and the words are where the weight is.

use clauseo_acp::{AcpEvent, AgentUpdate};
use clauseo_protocol::{ContentBlock, ThreadId};

use super::Held;

/// What the held output may weigh: four of the largest text chunk `clauseo_acp` lets through
/// (1 MiB), and minutes of a helper's ordinary words.
pub(in crate::actor::turn) const CAP_BYTES: usize = 4 * 1024 * 1024;

/// What a trim leaves, so the next one is a while off: one pass over the held output frees a
/// quarter of the cap.
const KEEP_BYTES: usize = CAP_BYTES / 4 * 3;

/// What one held event weighs beyond its text: about its own size in memory.
const EVENT_BYTES: usize = 256;

/// The text of `event` when it is words: an update (the lead's or a helper's) with a text chunk.
fn words(event: &AcpEvent) -> Option<&str> {
    let (AcpEvent::Update { update, .. } | AcpEvent::HelperUpdate { update, .. }) = event else {
        return None;
    };
    if let AgentUpdate::MessageChunk {
        content: ContentBlock::Text { text },
        ..
    } = update
    {
        Some(text)
    } else {
        None
    }
}

/// What `event` weighs: [`EVENT_BYTES`], plus its text when it is words.
pub(super) fn weight(event: &AcpEvent) -> usize {
    EVENT_BYTES + words(event).map_or(0, str::len)
}

/// A new pile-up starts in `held`, which holds nothing: its weight and its warning start over.
pub(super) fn new_pile(held: &mut Held) {
    held.bytes = 0;
    held.cap_warned = false;
}

/// Past [`CAP_BYTES`], drops the oldest words of `held` until it weighs at most
/// [`KEEP_BYTES`] or no words are left, and says so once per pile-up: the lead's answer, or a
/// helper's words, then has a gap.
pub(super) fn trim(held: &mut Held, thread: ThreadId) {
    if held.bytes <= CAP_BYTES {
        return;
    }
    let mut over = held.bytes - KEEP_BYTES;
    let mut dropped = 0_usize;
    let bytes = &mut held.bytes;
    held.updates.retain(|event| {
        if over == 0 || words(event).is_none() {
            return true;
        }
        let weight = weight(event);
        over = over.saturating_sub(weight);
        *bytes = bytes.saturating_sub(weight);
        dropped += 1;
        false
    });
    if dropped > 0 && !held.cap_warned {
        held.cap_warned = true;
        tracing::warn!(
            %thread,
            dropped,
            cap = CAP_BYTES,
            "agent output held for a turn passed its cap; its oldest words were dropped"
        );
    }
}

#[cfg(test)]
#[path = "cap_tests.rs"]
mod tests;
