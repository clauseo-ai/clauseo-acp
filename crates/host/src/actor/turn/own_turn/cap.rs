//! The cap on agent output held for a turn (`Held::updates`). While a close or an undo keeps
//! the agent's own turn from opening, a background helper's words pile up there chunk by chunk;
//! past [`CAP_BYTES`] the oldest updates go, down to [`KEEP_BYTES`], with a warning. Permission
//! requests and elicitations are never dropped: the agent waits for their answers.

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

/// What `event` weighs: [`EVENT_BYTES`], plus the text of an answer or thought chunk.
pub(super) fn weight(event: &AcpEvent) -> usize {
    let update =
        if let AcpEvent::Update { update, .. } | AcpEvent::HelperUpdate { update, .. } = event {
            Some(update)
        } else {
            None
        };
    let text = if let Some(AgentUpdate::MessageChunk {
        content: ContentBlock::Text { text },
        ..
    }) = update
    {
        text.len()
    } else {
        0
    };
    EVENT_BYTES + text
}

/// Past [`CAP_BYTES`], drops the oldest updates of `held` until it weighs at most
/// [`KEEP_BYTES`], and says how many went.
pub(super) fn trim(held: &mut Held, thread: ThreadId) {
    if held.bytes <= CAP_BYTES {
        return;
    }
    let mut over = held.bytes - KEEP_BYTES;
    let mut dropped = 0_usize;
    let bytes = &mut held.bytes;
    held.updates.retain(|event| {
        let update = matches!(
            event,
            AcpEvent::Update { .. } | AcpEvent::HelperUpdate { .. }
        );
        if over == 0 || !update {
            return true;
        }
        let weight = weight(event);
        over = over.saturating_sub(weight);
        *bytes = bytes.saturating_sub(weight);
        dropped += 1;
        false
    });
    tracing::warn!(
        %thread,
        dropped,
        cap = CAP_BYTES,
        "agent output held for a turn passed its cap; the oldest updates were dropped"
    );
}
