//! The order a thread's queued prompts go in, as the host runs them (decision record 0048,
//! P4b): first each prompt whose turn was put back at the head (`UserMessageEntry::at_head`),
//! the latest put back first, as each took the head when it was put back; then the rest in
//! `order`, the order they joined the queue. The host's start-up rebuild
//! (`crates/host/src/actor/turn/queue/restore.rs`) and the app's queue strip
//! (`ui/app/src/room/queue-picks.ts`) follow the same rule. A view with no `at_head` (an
//! older log) orders exactly by `order`.

use clauseo_protocol::{MessageId, UserMessageStatus, ViewEntry};

/// The messages of the queued prompts among `entries` (any order), in the order they go.
pub fn queued_order<'a>(entries: impl IntoIterator<Item = &'a ViewEntry>) -> Vec<MessageId> {
    let mut queued: Vec<(bool, clauseo_protocol::Seq, MessageId)> = entries
        .into_iter()
        .filter_map(|entry| {
            if let ViewEntry::UserMessage(prompt) = entry
                && prompt.status == UserMessageStatus::Queued
            {
                Some((prompt.at_head, prompt.order, prompt.message))
            } else {
                None
            }
        })
        .collect();
    queued.sort_by(|(head_a, order_a, _), (head_b, order_b, _)| {
        head_b.cmp(head_a).then_with(|| {
            if *head_a {
                order_b.cmp(order_a)
            } else {
                order_a.cmp(order_b)
            }
        })
    });
    queued.into_iter().map(|(_, _, message)| message).collect()
}
