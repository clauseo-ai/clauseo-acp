//! The install's hold on new sends as the turn path reads it: whether it applies, and every
//! queue shown again when it starts or ends (`turn/queue/held.rs`).

use super::super::Actor;
use crate::update::machine::Machine;

/// Whether new sends are held: an install under `now` waits for its cancelled turns.
pub(crate) fn gated(actor: &Actor) -> bool {
    #[cfg(test)]
    if TEST_GATE.get() {
        return true;
    }
    actor.update.machine.as_ref().is_some_and(Machine::gated)
}

#[cfg(test)]
thread_local! {
    /// A unit test holds new sends as an install under `now` would ([`hold_sends`]); the
    /// rig runs its actor on the test's own thread.
    static TEST_GATE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Holds new sends as an install under `now` does, or lifts that hold as taking the install
/// back does, with no machine: for unit tests of the turn path.
#[cfg(test)]
pub(crate) fn hold_sends(actor: &mut Actor, on: bool) {
    let was = gated(actor);
    TEST_GATE.set(on);
    if !on {
        super::super::turn::release(actor);
    }
    show_gate(actor, was);
}

/// The hold on new sends started or ended since it was `was` (an install under `now` stopping
/// its turns, its marker being written, the install taken back or begun): every queue shows
/// whether it waits for it (`queue_held_reason: update`).
pub(super) fn show_gate(actor: &mut Actor, was: bool) {
    if gated(actor) != was {
        super::super::turn::sync_all_held(actor);
    }
}
