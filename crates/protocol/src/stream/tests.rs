//! `StreamId` and `EntryId` text forms, in JSON and in `MessagePack`.

use std::str::FromStr;

use uuid::Uuid;

use crate::{
    CheckpointId, ElicitationId, EntryId, EventId, MatterId, MessageId, PermissionRequestId,
    StreamId, ThreadId, ToolCallId, TurnId,
};

const V7: &str = "01900000-0000-7000-8000-00000000002a";

fn every_entry_id() -> Vec<EntryId> {
    let id = Uuid::parse_str(V7).unwrap();
    vec![
        EntryId::Header(ThreadId::new(id).unwrap()),
        EntryId::UserMessage(MessageId::new(id).unwrap()),
        EntryId::AssistantMessage(MessageId::new(id).unwrap()),
        EntryId::AgentContent(EventId::new(id).unwrap()),
        EntryId::ToolCall(ToolCallId::new(id).unwrap()),
        EntryId::Permission(PermissionRequestId::new(id).unwrap()),
        EntryId::Elicitation(ElicitationId::new(id).unwrap()),
        EntryId::Step(TurnId::new(id).unwrap()),
        EntryId::LimitStop(TurnId::new(id).unwrap()),
        EntryId::Notice(EventId::new(id).unwrap()),
        EntryId::Marker(EventId::new(id).unwrap()),
        EntryId::MatterHeader(MatterId::new(id).unwrap()),
        EntryId::Version(CheckpointId::new(id).unwrap()),
    ]
}

#[test]
fn every_entry_id_round_trips_as_text() {
    let ids = every_entry_id();
    let kinds: Vec<_> = ids.iter().map(EntryId::kind).collect();
    assert_eq!(kinds, EntryId::KINDS);
    for id in ids {
        let text = format!("{}:{V7}", id.kind());
        assert_eq!(id.to_string(), text);
        assert_eq!(EntryId::from_str(&text).unwrap(), id);
        let json = serde_json::to_string(&id).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<EntryId>(&json).unwrap(), id);
        let packed = rmp_serde::to_vec_named(&id).unwrap();
        assert_eq!(packed, rmp_serde::to_vec_named(&text).unwrap());
        assert_eq!(rmp_serde::from_slice::<EntryId>(&packed).unwrap(), id);
    }
}

#[test]
fn bad_entry_ids_are_errors() {
    let v4 = V7.replacen('7', "4", 1);
    for bad in [
        String::new(),
        "tool_call".to_owned(),
        format!("toolcall:{V7}"),
        format!("step:{v4}"),
    ] {
        assert!(EntryId::from_str(&bad).is_err(), "{bad:?}");
    }
    assert!(serde_json::from_str::<EntryId>("{\"step\":1}").is_err());
}

#[test]
fn escaped_json_text_still_parses() {
    // serde_json hands an escaped string to `visit_str` as a copy; the result is the same.
    let json = format!("\"thread\\u003a{V7}\"");
    let stream = serde_json::from_str::<StreamId>(&json).unwrap();
    assert_eq!(stream.kind(), "thread");
}
