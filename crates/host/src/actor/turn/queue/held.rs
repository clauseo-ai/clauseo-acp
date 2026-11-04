//! The queue's held flag on screen: `queue_held` in the thread's header (and `ListThreads`),
//! true while the queue is held and turns wait. The flag is the host's alone; nothing logs it.

use clauseo_protocol::{StreamId, ThreadId};

use super::super::super::Actor;
use super::super::super::streams::StreamSlot;
use super::queue;
use crate::log_file::Cause;

/// Whether `thread`'s queue shows held: held with turns waiting.
pub(crate) fn queue_held(actor: &Actor, thread: ThreadId) -> bool {
    actor
        .threads
        .get(&thread)
        .is_some_and(|entry| entry.turn.queue.held && !entry.turn.queue.is_empty())
}

/// Shows `queue_held` when it changed: through the loaded view, or in the header kept for
/// `ListThreads` when the stream is not loaded; the app's thread rows compare it at the next
/// flush.
pub(in super::super) fn sync_held(actor: &mut Actor, thread: ThreadId) {
    let shown = queue_held(actor, thread);
    let Some(queue) = queue(actor, thread) else {
        return;
    };
    if queue.shown == shown {
        return;
    }
    queue.shown = shown;
    // The row `ListThreads` and `ThreadsChanged` carry shows it too.
    actor.lists.touch_thread(thread);
    let stream = StreamId::Thread(thread);
    let Actor {
        threads,
        streams,
        encoder,
        ..
    } = actor;
    match streams.get_mut(&stream) {
        Some(StreamSlot::Loaded(loaded)) => {
            let change = loaded
                .state
                .as_thread_mut()
                .and_then(|state| clauseo_reducer::set_queue_held(state, shown));
            let Some(change) = change else {
                return;
            };
            match loaded.view.publish(vec![change], encoder) {
                Ok(encoded) => {
                    for event in &encoded {
                        loaded.views.retain_mut(|sub| sub.offer(event));
                    }
                }
                Err(error) => {
                    tracing::error!(%thread, error = %Cause(&error), "queue_held was not shown");
                }
            }
        }
        Some(StreamSlot::Loading(_)) | None => {
            if let Some(header) = threads
                .get_mut(&thread)
                .and_then(|entry| entry.header.as_mut())
            {
                header.queue_held = shown;
            }
        }
    }
}
