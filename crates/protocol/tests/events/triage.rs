//! `ThreadSnoozed` and `ThreadDone` (the rail's triage, 0045) and their open enums `SnoozeBy`
//! and `DoneBy`: wire forms, round trips in JSON and `MessagePack`, and a value from a newer
//! Clauseo; the triage commands' wire forms.

use clauseo_protocol::{
    CommandEnvelope, DomainEvent, DoneBy, HostCommand, SnoozeBy, ThreadDone, ThreadSnoozed,
    Timestamp,
};
use serde_json::json;

use crate::fixtures::thread_id;

fn round_trips(event: &DomainEvent) -> serde_json::Value {
    let json = serde_json::to_value(event).unwrap();
    assert_eq!(
        &serde_json::from_value::<DomainEvent>(json.clone()).unwrap(),
        event
    );
    let packed = rmp_serde::to_vec_named(event).unwrap();
    assert_eq!(
        &rmp_serde::from_slice::<DomainEvent>(&packed).unwrap(),
        event
    );
    json
}

#[test]
fn a_snooze_writes_its_until_and_a_cleared_one_leaves_it_out() {
    let set: DomainEvent = ThreadSnoozed {
        until: Some(Timestamp::new(1_760_086_400_000).unwrap()),
        by: SnoozeBy::Lawyer,
    }
    .into();
    assert_eq!(
        round_trips(&set),
        json!({"thread_snoozed": {"until": 1_760_086_400_000_i64, "by": "lawyer"}})
    );
    let woke: DomainEvent = ThreadSnoozed {
        until: None,
        by: SnoozeBy::Expired,
    }
    .into();
    assert_eq!(
        round_trips(&woke),
        json!({"thread_snoozed": {"by": "expired"}})
    );
}

#[test]
fn done_writes_both_fields() {
    let done: DomainEvent = ThreadDone {
        done: true,
        by: DoneBy::MorningSweep,
    }
    .into();
    assert_eq!(
        round_trips(&done),
        json!({"thread_done": {"done": true, "by": "morning_sweep"}})
    );
    // `by` is required: an entry without it does not decode.
    let missing = json!({"thread_done": {"done": true}});
    assert!(serde_json::from_value::<DomainEvent>(missing).is_err());
}

#[test]
fn a_by_from_a_newer_clauseo_decodes_to_unknown_and_writes_back() {
    let read: DomainEvent =
        serde_json::from_value(json!({"thread_done": {"done": false, "by": "assistant"}})).unwrap();
    let DomainEvent::ThreadDone(done) = &read else {
        panic!("not thread_done: {read:?}");
    };
    assert_eq!(done.by, DoneBy::Unknown);
    let written = serde_json::to_value(&read).unwrap();
    assert_eq!(written["thread_done"]["by"], json!("unknown"));
    assert_eq!(round_trips(&read), written);

    let read: DomainEvent =
        serde_json::from_value(json!({"thread_snoozed": {"by": "calendar"}})).unwrap();
    let DomainEvent::ThreadSnoozed(snoozed) = &read else {
        panic!("not thread_snoozed: {read:?}");
    };
    assert_eq!((snoozed.until, snoozed.by), (None, SnoozeBy::Unknown));
}

#[test]
fn the_triage_commands_decode_from_their_wire_forms() {
    let thread = serde_json::to_value(thread_id(2)).unwrap();
    let id = "01900000-0000-7000-8000-000000000009";
    let cases = [
        (
            json!({"snooze_thread": {"thread": thread, "until": 1_760_086_400_000_i64}}),
            HostCommand::SnoozeThread {
                thread: thread_id(2),
                until: Some(Timestamp::new(1_760_086_400_000).unwrap()),
            },
        ),
        (
            json!({"snooze_thread": {"thread": thread}}),
            HostCommand::SnoozeThread {
                thread: thread_id(2),
                until: None,
            },
        ),
        (
            json!({"mark_thread_done": {"thread": thread, "done": false}}),
            HostCommand::MarkThreadDone {
                thread: thread_id(2),
                done: false,
            },
        ),
        (
            json!({"sweep_done": {"day_start": 1_759_968_000_000_i64}}),
            HostCommand::SweepDone {
                day_start: Timestamp::new(1_759_968_000_000).unwrap(),
            },
        ),
    ];
    for (wire, command) in cases {
        let envelope: CommandEnvelope =
            serde_json::from_value(json!({"command_id": id, "command": wire.clone()})).unwrap();
        assert_eq!(envelope.command, command);
        assert_eq!(serde_json::to_value(&command).unwrap(), wire);
        assert!(envelope.validate().is_ok());
    }
    // Commands refuse a field they do not know.
    let extra = json!({"command_id": id, "command": {"sweep_done": {"day_start": 1, "tz": "UTC"}}});
    assert!(serde_json::from_value::<CommandEnvelope>(extra).is_err());
}
