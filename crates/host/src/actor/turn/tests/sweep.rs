//! A cancelled turn's sweep (`cancel::sweep_leftovers`) takes the prompt's listing while the host
//! serves, and leaves it while the host stops: the stop's own kill covers the agent's tree then.

use clauseo_protocol::{StopReason, ThreadId};

use super::rig::Rig;
use crate::actor::HostPhase;
use crate::actor::turn::{BeforePrompt, cancel};

/// Gives `thread`'s current turn an (empty) prompt listing; whether it is still there after.
fn listing_left(rig: &mut Rig, thread: ThreadId, stop: StopReason) -> bool {
    let current = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_mut();
    current.unwrap().before_prompt = Some(BeforePrompt::default());
    cancel::sweep_leftovers(&mut rig.actor, thread, stop);
    let current = rig
        .actor
        .threads
        .get_mut(&thread)
        .unwrap()
        .turn
        .current
        .as_ref();
    current.unwrap().before_prompt.is_some()
}

#[test]
fn a_cancelled_turn_takes_its_listing_while_the_host_serves() {
    let mut rig = Rig::start("sweep-serving", "");
    let thread = rig.thread();
    rig.running_turn(thread);
    assert!(!listing_left(&mut rig, thread, StopReason::Cancelled));
    // A turn that ends any other way, not cancelled, sweeps nothing.
    assert!(listing_left(&mut rig, thread, StopReason::EndTurn));
    rig.finish();
}

#[test]
fn no_sweep_while_the_host_stops() {
    let mut rig = Rig::start("sweep-stopping", "");
    let thread = rig.thread();
    rig.running_turn(thread);
    let serving = std::mem::replace(&mut rig.actor.phase, HostPhase::Stopping(Vec::new()));
    let left = listing_left(&mut rig, thread, StopReason::Cancelled);
    rig.actor.phase = serving;
    assert!(left, "the sweep took the listing while the host stopped");
    rig.finish();
}
