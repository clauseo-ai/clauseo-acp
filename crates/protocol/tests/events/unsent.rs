//! Why a turn that never started ended (`TurnEnded.cause`): a cause and no cause both
//! round-trip, a `TurnEnded` written before the field reads as no cause, and a cause a newer
//! host adds reads as `Unknown`. The message's status `not_sent` is written as that word.

use clauseo_protocol::{DomainEvent, StopReason, TurnEnded, UnsentCause, UserMessageStatus};
use serde_json::json;

use crate::fixtures::{round_trips, turn_id};

fn ended(cause: Option<UnsentCause>) -> DomainEvent {
    TurnEnded {
        turn: turn_id(3),
        stop: StopReason::Cancelled,
        usage: None,
        checkpoint_after: None,
        code_mode: false,
        cause,
    }
    .into()
}

#[test]
fn turn_ended_with_and_without_a_cause_round_trips() {
    let causes = [
        UnsentCause::Stopped,
        UnsentCause::Removed,
        UnsentCause::SentNow,
        UnsentCause::SignIn,
        UnsentCause::Update,
        UnsentCause::NotSent,
    ];
    for cause in causes {
        round_trips(&ended(Some(cause)));
    }
    round_trips(&ended(None));

    let sent_now = serde_json::to_value(ended(Some(UnsentCause::SentNow))).unwrap();
    assert_eq!(sent_now["turn_ended"]["cause"], json!("sent_now"));
    let none = serde_json::to_value(ended(None)).unwrap();
    assert_eq!(none["turn_ended"].get("cause"), None, "{none}");
}

#[test]
fn a_turn_ended_without_the_field_reads_no_cause() {
    let old = json!({
        "turn_ended": {
            "turn": serde_json::to_value(turn_id(3)).unwrap(),
            "stop": "cancelled",
        }
    });
    let read: DomainEvent = serde_json::from_value(old).unwrap();
    assert_eq!(read, ended(None));
}

#[test]
fn a_cause_from_a_newer_host_reads_as_unknown() {
    let newer = json!({
        "turn_ended": {
            "turn": serde_json::to_value(turn_id(3)).unwrap(),
            "stop": "cancelled",
            "cause": "something_new",
        }
    });
    let read: DomainEvent = serde_json::from_value(newer).unwrap();
    assert_eq!(read, ended(Some(UnsentCause::Unknown)));
}

#[test]
fn the_not_sent_status_round_trips_as_not_sent() {
    round_trips(&UserMessageStatus::NotSent);
    assert_eq!(
        serde_json::to_value(UserMessageStatus::NotSent).unwrap(),
        json!("not_sent")
    );
}
