//! How a prompt's delivery is read: a missing one is queue, queue has one wire form in the
//! command, the logged `UserMessage` and its view entry, and an unknown mode is refused. Shared
//! helpers are in `commands.rs`.

use clauseo_protocol::{
    CommandEnvelope, ContentBlock, Delivery, DeliveryRequest, HostCommand, RawPayload, Seq,
    UserMessage, UserMessageEntry, UserMessageStatus,
};

use super::fixtures::{entry, message_id, packed_raw, round_trips, thread_id, turn_id};
use super::{Validate, envelope, send_prompt, send_prompt_json, text};

#[test]
fn send_prompt_without_delivery_is_queue() {
    let json = send_prompt_json();
    let decoded: CommandEnvelope = serde_json::from_str(&json).unwrap();
    let packed = rmp_serde::to_vec_named(&decoded).unwrap();
    let unpacked: CommandEnvelope = rmp_serde::from_slice(&packed).unwrap();
    // Queue is written back as a missing field, never `null` or `"queue"`.
    assert_eq!(serde_json::to_string(&decoded).unwrap(), json);
    for command in [decoded.command, unpacked.command] {
        assert!(matches!(
            command,
            HostCommand::SendPrompt {
                delivery: DeliveryRequest::Queue,
                ..
            }
        ));
        assert_eq!(command.delivery(), Some(Delivery::Queue));
        assert!(command.validate().is_ok());
    }
    // A delivery that is there is read as given.
    let steer = json.replacen("}]}", r#"}],"delivery":"steer"}"#, 1);
    let command: CommandEnvelope = serde_json::from_str(&steer).unwrap();
    assert_eq!(command.command.delivery(), Some(Delivery::Steer));
    // Commands without a prompt have no delivery.
    let cancel = HostCommand::Cancel {
        thread: thread_id(1),
    };
    assert_eq!(cancel.delivery(), None);
}

/// Queue has one wire form: an explicit `"queue"` reads as queue and is written back as a missing
/// field, in the command, the logged `UserMessage` and its view entry, so two bodies that differ
/// only in this never differ once decoded.
#[test]
fn queue_has_one_wire_form() {
    let explicit = send_prompt_json().replacen("}]}", r#"}],"delivery":"queue"}"#, 1);
    let decoded: CommandEnvelope = serde_json::from_str(&explicit).unwrap();
    assert_eq!(serde_json::to_string(&decoded).unwrap(), send_prompt_json());
    let packed = rmp_serde::to_vec_named(&decoded).unwrap();
    let missing = rmp_serde::to_vec_named(&envelope(9, send_prompt(vec![text("hi")]))).unwrap();
    assert_eq!(packed, missing);

    let message = |delivery: Delivery| UserMessage {
        turn: turn_id(3),
        message: message_id(5),
        acp_message_id: None,
        content: vec![ContentBlock::Text {
            text: "hi".to_owned(),
        }],
        delivery,
    };
    let entry = |delivery: Delivery| UserMessageEntry {
        order: Seq::FIRST,
        message: message_id(5),
        turn: turn_id(3),
        content: Vec::new(),
        delivery,
        status: UserMessageStatus::Queued,
        edited: false,
        steer: None,
    };
    let queued = serde_json::to_string(&message(Delivery::Queue)).unwrap();
    assert!(!queued.contains("delivery"), "{queued}");
    let explicit = queued.strip_suffix('}').unwrap().to_owned() + r#","delivery":"queue"}"#;
    let read: UserMessage = serde_json::from_str(&explicit).unwrap();
    assert_eq!(read, message(Delivery::Queue));
    let steered = serde_json::to_string(&message(Delivery::Steer)).unwrap();
    assert!(steered.ends_with(r#""delivery":"steer"}"#), "{steered}");
    let queued = serde_json::to_string(&entry(Delivery::Queue)).unwrap();
    assert!(!queued.contains("delivery"), "{queued}");
    let read: UserMessageEntry = serde_json::from_str(&queued).unwrap();
    assert_eq!(read, entry(Delivery::Queue));
    let sent_now = serde_json::to_string(&entry(Delivery::SendNow)).unwrap();
    assert!(sent_now.contains(r#""delivery":"send_now""#), "{sent_now}");
    for delivery in [Delivery::Queue, Delivery::SendNow, Delivery::Steer] {
        round_trips(&message(delivery));
        round_trips(&entry(delivery));
    }
}

/// A delivery mode this version does not know fails to decode, in JSON and in `MessagePack`, so
/// it is never run as another mode; each known one decodes to its `Delivery`.
#[test]
fn unknown_delivery_refused_on_decode() {
    let json = send_prompt_json().replacen("}]}", r#"}],"delivery":"teleport"}"#, 1);
    let decoded = serde_json::from_str::<CommandEnvelope>(&json);
    assert!(
        decoded
            .as_ref()
            .is_err_and(|error| error.to_string().contains("unknown variant `teleport`")),
        "{decoded:?}"
    );
    // The same in MessagePack: pack a known delivery, then rename it.
    let steer = envelope(
        9,
        HostCommand::SendPrompt {
            thread: thread_id(1),
            content: vec![text("hi")],
            delivery: DeliveryRequest::Steer,
        },
    );
    let mut packed = packed_raw(&steer);
    *entry(
        entry(entry(&mut packed, "command"), "send_prompt"),
        "delivery",
    ) = RawPayload::Str("teleport".into());
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    let unpacked = rmp_serde::from_slice::<CommandEnvelope>(&bytes);
    assert!(
        unpacked
            .as_ref()
            .is_err_and(|error| error.to_string().contains("teleport")),
        "{unpacked:?}"
    );
    let known = [
        (DeliveryRequest::Queue, Delivery::Queue),
        (DeliveryRequest::SendNow, Delivery::SendNow),
        (DeliveryRequest::Steer, Delivery::Steer),
    ];
    for (request, logged) in known {
        assert_eq!(Delivery::from(request), logged);
        let command = HostCommand::SendPrompt {
            thread: thread_id(1),
            content: vec![text("hi")],
            delivery: request,
        };
        assert_eq!(command.delivery(), Some(logged));
        round_trips(&command);
        // The same wire form as the logged delivery.
        assert_eq!(
            serde_json::to_string(&request).unwrap(),
            serde_json::to_string(&logged).unwrap()
        );
    }
}
