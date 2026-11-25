//! A Stop with no turn running (`cancel.rs`): helpers working on in the background past their
//! turn end `Stopped` and the agent hears `session/cancel`; with none open, only the receipt.

use clauseo_protocol::{CommandOutcome, DomainEvent, HelperOutcome, HostCommand, ViewEntry};

use super::agents::send_prompt;
use super::failures::until_ended;
use super::helpers::{CANCEL, one_turn, story};
use super::helpers_ends::{helper_entries, open_start};
use super::rig::Rig;
use crate::actor::turn::helpers as turn_helpers;

/// A helper still working in the background after its turn ended: the lawyer's Stop ends it
/// `Stopped` in the receipt's write, its card closes, and the agent hears `session/cancel`.
#[test]
fn a_stop_with_no_turn_running_stops_an_open_background_helper() {
    let steps = format!("{}{CANCEL}\n", one_turn("end_turn"));
    let mut rig = Rig::start("helpers-idle-stop", &steps);
    let thread = rig.thread();
    let turn = send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    turn_helpers::start(&mut rig.actor, thread, turn, open_start()).unwrap();
    rig.settle_commits();
    let stopped = rig.dispatch(11, HostCommand::Cancel { thread });
    assert!(matches!(stopped, Ok(CommandOutcome::Done)), "{stopped:?}");
    rig.settle_commits();
    assert_eq!(
        story(&rig.log(thread))[1..],
        [
            "turn_ended EndTurn",
            "start paper-reader call= model=- label=- task=true",
            "end paper-reader Stopped -",
        ]
    );
    assert!(!turn_helpers::any_open(&rig.actor, thread));
    let entries = helper_entries(&mut rig, thread);
    let [ViewEntry::Helper(entry)] = entries.as_slice() else {
        panic!("{entries:?}");
    };
    assert_eq!(
        entry.outcome,
        Some(HelperOutcome::Stopped),
        "the card closes"
    );
    rig.finish();
}

/// With no helper open, a Stop with no turn running logs nothing but its receipt and sends the
/// agent nothing (the scenario expects no `session/cancel`).
#[test]
fn a_stop_with_no_turn_and_no_helper_is_a_receipt_only() {
    let mut rig = Rig::start("helpers-idle-stop-none", &one_turn("end_turn"));
    let thread = rig.thread();
    send_prompt(&mut rig, 10, thread);
    until_ended(&mut rig, thread);
    rig.settle_commits();
    let before = rig.log(thread);
    let stopped = rig.dispatch(11, HostCommand::Cancel { thread });
    assert!(matches!(stopped, Ok(CommandOutcome::Done)), "{stopped:?}");
    rig.settle_commits();
    let after = rig.log(thread);
    assert_eq!(after, before, "nothing but the receipt");
    assert!(
        !after
            .iter()
            .any(|event| matches!(event, DomainEvent::HelperEnded(_)))
    );
    rig.finish();
}
