//! The thread's triage: the lawyer's snooze and the done mark (`ThreadSnoozed`, `ThreadDone`),
//! both kept on the header, and the wake rule: an event that asks for the lawyer again (their
//! own message, a permission request, an elicitation, a limit stop, a failed or ended turn)
//! clears both. The rule reads no clock: a snooze's time passing is the host's `ThreadSnoozed`
//! with `by: expired`, which clears the snooze as any `until: None` does.

use clauseo_protocol::{DomainEvent, ThreadDone, ThreadSnoozed, ViewEntry};

use super::{set, update_header};
use crate::Applied;
use crate::change::ViewChange;
use crate::state::ThreadState;

/// `ThreadSnoozed`: the header's `snoozed_until` becomes `until` (`None` clears it).
pub(super) fn snoozed(state: &mut ThreadState, event: &ThreadSnoozed, out: &mut Applied) {
    update_header(state, "thread_snoozed", out, |h| {
        set(&mut h.snoozed_until, event.until)
    });
}

/// `ThreadDone`: the header's `done` becomes the event's.
pub(super) fn done(state: &mut ThreadState, event: ThreadDone, out: &mut Applied) {
    update_header(state, "thread_done", out, |h| set(&mut h.done, event.done));
}

/// Whether `event` wakes the thread: it asks for the lawyer again, or is the lawyer's own
/// message.
pub(crate) const fn wakes(event: &DomainEvent) -> bool {
    matches!(
        event,
        DomainEvent::UserMessage(_)
            | DomainEvent::PermissionRequested(_)
            | DomainEvent::ElicitationRequested(_)
            | DomainEvent::TurnStoppedAtLimit(_)
            | DomainEvent::TurnFailed(_)
            | DomainEvent::TurnEnded(_)
    )
}

/// Before `event` is folded: clears the header's snooze and done mark when `event` wakes the
/// thread and either is set, and says whether it did. The header's `Upsert` is left to
/// [`after_wake`], so an event that changes the header itself sends one, not two.
pub(super) fn wake(state: &mut ThreadState, event: &DomainEvent) -> bool {
    if !wakes(event) {
        return false;
    }
    let Some(header) = state.header.as_mut() else {
        return false;
    };
    let changed = header.done || header.snoozed_until.is_some();
    header.done = false;
    header.snoozed_until = None;
    changed
}

/// After a waking event is folded: the header's `Upsert`, unless the event sent one since
/// `from` (the length of `out.changes` before it), which already holds the cleared fields.
pub(super) fn after_wake(state: &ThreadState, from: usize, out: &mut Applied) {
    let sent = out.changes.iter().skip(from).any(|change| {
        matches!(change, ViewChange::Upsert(entry) if matches!(**entry, ViewEntry::Header(_)))
    });
    if !sent && let Some(header) = &state.header {
        out.changes
            .push(ViewChange::Upsert(Box::new(ViewEntry::Header(
                header.clone(),
            ))));
    }
}
