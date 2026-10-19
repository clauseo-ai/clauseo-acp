//! Events: the envelope and `DomainEvent` on the wire, in JSON and in `MessagePack`, and the
//! enums that decode values a newer Clauseo may add.

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "support/examples.rs"]
pub mod examples;

#[cfg(test)]
#[path = "events/unknown_variants.rs"]
mod unknown_variants;

use clauseo_protocol::{
    DomainEvent, ElicitationRequest, ElicitationRequested, Envelope, PromptBlock, RawPayload,
    SCHEMA_VERSION, StopReason, StreamId, ToolCallStarted, TurnEnded, TurnStarted,
};

use examples::{every_event, tool_call_started};
use fixtures::{
    checkpoint_id, decodes_in_both, elicitation_id, entry, envelope, fails_in_both, matter_id,
    packed_raw, raw, remove_entry, round_trips, thread_id, turn_id, v7_text,
};
use uuid::Uuid;

/// An unknown event: unsorted keys, nested lists, a negative and a large number, a float.
const LATER_EVENT: &str =
    r#"{"later_kind":{"z":1,"a":[true,null,{"y":-2,"b":18446744073709551615}],"m":0.5}}"#;

#[test]
fn stream_id_is_a_plain_string() {
    let thread = StreamId::Thread(thread_id(1));
    let matter = StreamId::Matter(matter_id(2));
    let cases = [
        (thread, format!("thread:{}", v7_text(1))),
        (matter, format!("matter:{}", v7_text(2))),
    ];
    for (stream, text) in cases {
        assert_eq!(stream.to_string(), text);
        assert_eq!(text.parse::<StreamId>().unwrap(), stream);
        let json = serde_json::to_string(&stream).unwrap();
        assert_eq!(json, format!("\"{text}\""));
        assert_eq!(serde_json::from_str::<StreamId>(&json).unwrap(), stream);
        // MessagePack carries the same text, as a str, not a map or a byte array.
        let packed = rmp_serde::to_vec_named(&stream).unwrap();
        assert_eq!(packed, rmp_serde::to_vec_named(&text).unwrap());
        assert_eq!(rmp_serde::from_slice::<StreamId>(&packed).unwrap(), stream);
    }
    let nil_v4 = "matter:00000000-0000-4000-8000-000000000000";
    for bad in ["thread", "topic:x", "thread:not-a-uuid", nil_v4] {
        assert!(bad.parse::<StreamId>().is_err(), "{bad} parsed");
        let json = format!("\"{bad}\"");
        assert!(
            serde_json::from_str::<StreamId>(&json).is_err(),
            "{bad} decoded"
        );
    }
}

#[test]
fn unknown_kind_decodes_to_unknown() {
    let expected = DomainEvent::Unknown {
        kind: "later_kind".to_owned(),
        payload: raw(r#"{"z":1,"a":[true,null,{"y":-2,"b":18446744073709551615}],"m":0.5}"#),
    };
    decodes_in_both(LATER_EVENT, &expected);
    assert_eq!(expected.kind(), "later_kind");
    assert!(!DomainEvent::KINDS.contains(&"later_kind"));

    // Inside an envelope it still decodes, so `seq` advances past it.
    let json = format!(
        r#"{{"schema_version":1,"event_id":"{}","stream":"thread:{}","seq":8,"ts":0,"event":{LATER_EVENT}}}"#,
        v7_text(40),
        v7_text(2)
    );
    let read: Envelope = serde_json::from_str(&json).unwrap();
    assert_eq!(read.seq.get(), 8);
    assert_eq!(read.event, expected);

    // Not a one-key object: an error, not `Unknown`.
    for bad in ["{}", r#"{"a":{},"b":{}}"#, r#""later_kind""#, "[]", "3"] {
        fails_in_both::<DomainEvent>(bad);
    }
}

#[test]
fn unknown_kind_round_trips_verbatim() {
    let event: DomainEvent = serde_json::from_str(LATER_EVENT).unwrap();
    assert_eq!(serde_json::to_string(&event).unwrap(), LATER_EVENT);

    let json = format!(
        r#"{{"schema_version":1,"event_id":"{}","stream":"matter:{}","seq":3,"ts":-5,"event":{LATER_EVENT}}}"#,
        v7_text(40),
        v7_text(1)
    );
    let read: Envelope = serde_json::from_str(&json).unwrap();
    let written = serde_json::to_string(&read).unwrap();
    assert_eq!(written, json);
    // Never written under a kind named `unknown`.
    assert!(!written.contains("\"unknown\""), "{written}");
}

#[test]
fn unknown_kind_round_trips_verbatim_in_messagepack() {
    // The stored form R7 writes: a named-map envelope with binary ids, and a payload holding
    // binary data, an integer key and keys in no sorted order.
    let payload = RawPayload::Map(vec![
        (
            RawPayload::Str("z".to_owned()),
            RawPayload::Bytes(vec![0, 159, 255]),
        ),
        (RawPayload::U64(7), RawPayload::I64(-9)),
        (RawPayload::Str("a".to_owned()), RawPayload::F64(1.5)),
    ]);
    let event = DomainEvent::Unknown {
        kind: "later_kind".to_owned(),
        payload,
    };
    let original = rmp_serde::to_vec_named(&envelope(event.clone())).unwrap();
    let read: Envelope = rmp_serde::from_slice(&original).unwrap();
    assert_eq!(read.event, event);
    assert_eq!(rmp_serde::to_vec_named(&read).unwrap(), original);

    // The event alone is a one-entry map keyed by its kind, as in JSON.
    let alone = rmp_serde::to_vec_named(&event).unwrap();
    let as_raw: RawPayload = rmp_serde::from_slice(&alone).unwrap();
    let RawPayload::Map(entries) = as_raw else {
        panic!("not a map: {as_raw:?}");
    };
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].0, RawPayload::Str("later_kind".to_owned()));

    // `serde_json::Value` cannot hold the binary payload, which is why `RawPayload` exists.
    assert!(rmp_serde::from_slice::<serde_json::Value>(&alone).is_err());
}

#[test]
fn elicitation_schema_round_trips_in_messagepack() {
    let schema_json = r#"{"type":"object","required":["party"],"properties":{"party":{"type":"string","maxLength":200},"amount":{"type":"number"}}}"#;
    let event = DomainEvent::from(ElicitationRequested {
        elicitation: elicitation_id(12),
        request: ElicitationRequest::Form {
            message: "Which party?".to_owned(),
            schema: raw(schema_json),
        },
    });
    let packed = rmp_serde::to_vec_named(&envelope(event.clone())).unwrap();
    let read: Envelope = rmp_serde::from_slice(&packed).unwrap();
    assert_eq!(read.event, event);
    assert_eq!(rmp_serde::to_vec_named(&read).unwrap(), packed);

    // The schema's keys keep their order through MessagePack and back to JSON.
    let DomainEvent::ElicitationRequested(requested) = &read.event else {
        panic!("decoded to {:?}", read.event);
    };
    let ElicitationRequest::Form { schema, .. } = &requested.request else {
        panic!("decoded to {:?}", requested.request);
    };
    assert_eq!(schema.to_json_text().unwrap(), schema_json);
    // `serde_json::Value` would sort them.
    let sorted = serde_json::from_str::<serde_json::Value>(schema_json).unwrap();
    assert_ne!(sorted.to_string(), schema_json);
}

#[test]
fn none_fields_are_omitted() {
    let mut started = tool_call_started();
    started.tool_name = None;
    started.raw_input = None;
    started.raw_output = None;
    let ended = TurnEnded {
        turn: turn_id(3),
        stop: StopReason::Cancelled,
        usage: None,
        checkpoint_after: None,
    };
    let mut quiet = envelope(ended.into());
    quiet.command_id = None;
    let json = serde_json::to_string(&quiet).unwrap();
    for key in ["usage", "checkpoint_after", "command_id"] {
        assert!(!json.contains(&format!("\"{key}\"")), "{key} in {json}");
    }
    assert!(!json.contains("null"), "{json}");

    let json = serde_json::to_string(&DomainEvent::from(Box::new(started))).unwrap();
    for key in ["tool_name", "raw_input", "raw_output"] {
        assert!(!json.contains(&format!("\"{key}\"")), "{key} in {json}");
    }
    assert!(!json.contains("null"), "{json}");

    // In MessagePack a skipped field is a missing key, and it reads back as `None`.
    let packed = rmp_serde::to_vec_named(&quiet).unwrap();
    let keys: RawPayload = rmp_serde::from_slice(&packed).unwrap();
    let RawPayload::Map(entries) = keys else {
        panic!("not a map: {keys:?}");
    };
    assert!(
        !entries
            .iter()
            .any(|(k, _)| *k == RawPayload::Str("command_id".to_owned()))
    );
    assert_eq!(rmp_serde::from_slice::<Envelope>(&packed).unwrap(), quiet);

    // A field that holds a value is written.
    let json = serde_json::to_string(&envelope(
        TurnEnded {
            checkpoint_after: Some(checkpoint_id(14)),
            ..ended
        }
        .into(),
    ))
    .unwrap();
    assert!(json.contains("\"checkpoint_after\"") && json.contains("\"command_id\""));
}

#[test]
fn known_kind_with_bad_payload_is_an_error() {
    let good = format!(
        r#"{{"turn_started":{{"turn":"{}","checkpoint_before":"{}"}}}}"#,
        v7_text(3),
        v7_text(8)
    );
    let expected = DomainEvent::from(TurnStarted {
        turn: turn_id(3),
        checkpoint_before: checkpoint_id(8),
    });
    assert_eq!(
        serde_json::from_str::<DomainEvent>(&good).unwrap(),
        expected
    );

    let missing = format!(r#"{{"turn_started":{{"turn":"{}"}}}}"#, v7_text(3));
    let bad_id = r#"{"turn_started":{"turn":"not-a-uuid","checkpoint_before":"x"}}"#;
    let v4 = "00000000-0000-4000-8000-000000000001";
    let not_v7 = format!(r#"{{"turn_started":{{"turn":"{v4}","checkpoint_before":"{v4}"}}}}"#);
    for bad in [
        missing.as_str(),
        bad_id,
        not_v7.as_str(),
        r#"{"turn_started":"x"}"#,
        r#"{"turn_started":null}"#,
        r#"{"thread_renamed":{"title":"t","by":7}}"#,
        r#"{"thread_renamed":{"by":"lawyer"}}"#,
    ] {
        let read = serde_json::from_str::<DomainEvent>(bad);
        assert!(read.is_err(), "JSON {bad} decoded to {read:?}");
    }

    // The same in MessagePack, starting from what `to_vec_named` writes for the good event.
    let good_packed = packed_raw(&expected);
    let bad_payloads = [
        (|payload: &mut RawPayload| remove_entry(payload, "checkpoint_before")) as fn(&mut _),
        |payload| *entry(payload, "turn") = RawPayload::Bytes(vec![7; 15]),
        |payload| *entry(payload, "turn") = RawPayload::Bytes(Uuid::nil().as_bytes().to_vec()),
        |payload| *payload = RawPayload::Str("x".to_owned()),
        |payload| *payload = RawPayload::Null,
    ];
    let bytes = rmp_serde::to_vec_named(&good_packed).unwrap();
    assert_eq!(
        rmp_serde::from_slice::<DomainEvent>(&bytes).unwrap(),
        expected
    );
    for corrupt in bad_payloads {
        let mut packed = good_packed.clone();
        corrupt(entry(&mut packed, "turn_started"));
        let bytes = rmp_serde::to_vec_named(&packed).unwrap();
        let read = rmp_serde::from_slice::<DomainEvent>(&bytes);
        assert!(read.is_err(), "MessagePack {packed:?} decoded to {read:?}");
    }
}

#[test]
fn domain_event_kind_is_the_key() {
    let events = every_event();
    let kinds: Vec<&str> = events.iter().map(DomainEvent::kind).collect();
    assert_eq!(
        kinds,
        DomainEvent::KINDS,
        "every_event covers every kind in order"
    );
    let mut unique = kinds.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), kinds.len(), "kinds are distinct");
    assert_eq!(DomainEvent::KINDS.len(), 36);

    for event in &events {
        let json: RawPayload =
            serde_json::from_str(&serde_json::to_string(event).unwrap()).unwrap();
        let RawPayload::Map(entries) = json else {
            panic!("{} is not a map", event.kind());
        };
        assert_eq!(entries.len(), 1, "{}", event.kind());
        assert_eq!(entries[0].0, RawPayload::Str(event.kind().to_owned()));
    }
    let archived = serde_json::to_string(&events[2]).unwrap();
    assert_eq!(archived, r#"{"thread_archived":{}}"#);

    let unknown: DomainEvent = serde_json::from_str(LATER_EVENT).unwrap();
    assert_eq!(unknown.kind(), "later_kind");
}

#[test]
fn every_kind_round_trips_in_json_and_messagepack() {
    for event in every_event() {
        round_trips(&event);
        round_trips(&envelope(event));
    }
}

#[test]
fn envelope_writes_the_schema_version() {
    let written = envelope(
        TurnStarted {
            turn: turn_id(3),
            checkpoint_before: checkpoint_id(8),
        }
        .into(),
    );
    assert_eq!(written.schema_version, SCHEMA_VERSION);
    let json = serde_json::to_string(&written).unwrap();
    let expected = format!(
        r#"{{"schema_version":1,"event_id":"{}","stream":"thread:{}","seq":7,"ts":1760000000000,"command_id":"{}","event":{{"turn_started":{{"turn":"{}","checkpoint_before":"{}"}}}}}}"#,
        v7_text(40),
        v7_text(2),
        v7_text(41),
        v7_text(3),
        v7_text(8)
    );
    assert_eq!(json, expected);
    // `seq` 0 is not a position.
    let zero = expected.replace(r#""seq":7"#, r#""seq":0"#);
    assert!(serde_json::from_str::<Envelope>(&zero).is_err());
    let mut packed = packed_raw(&written);
    *entry(&mut packed, "seq") = RawPayload::U64(0);
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    assert!(rmp_serde::from_slice::<Envelope>(&bytes).is_err());
}

#[test]
fn newer_optional_field_is_ignored() {
    let json = format!(
        r#"{{"schema_version":1,"event_id":"{}","stream":"thread:{}","seq":7,"ts":0,"later":[1],"event":{{"turn_ended":{{"turn":"{}","stop":"end_turn","later_field":{{"x":1}}}}}}}}"#,
        v7_text(40),
        v7_text(2),
        v7_text(3)
    );
    let read: Envelope = serde_json::from_str(&json).unwrap();
    let expected = DomainEvent::from(TurnEnded {
        turn: turn_id(3),
        stop: StopReason::EndTurn,
        usage: None,
        checkpoint_after: None,
    });
    assert_eq!(read.event, expected);
    // In MessagePack: what `to_vec_named` writes, plus keys this version does not know.
    let mut packed = packed_raw(&read);
    let later = |map: &mut RawPayload| {
        let RawPayload::Map(entries) = map else {
            panic!("not a map: {map:?}");
        };
        entries.push((RawPayload::Str("later".to_owned()), raw(r#"{"x":[1]}"#)));
    };
    later(&mut packed);
    later(entry(entry(&mut packed, "event"), "turn_ended"));
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    assert_eq!(
        rmp_serde::from_slice::<Envelope>(&bytes).unwrap().event,
        expected
    );

    // A nested payload ignores them too.
    let mut started = serde_json::to_value(tool_call_started()).unwrap();
    started["later_field"] = serde_json::json!("x");
    started["locations"][0]["inside"]["later_field"] = serde_json::json!(1);
    let read: ToolCallStarted = serde_json::from_value(started).unwrap();
    assert_eq!(read, tool_call_started());

    // Commands, unlike events, refuse fields they do not know.
    assert!(serde_json::from_str::<PromptBlock>(r#"{"text":{"text":"hi"}}"#).is_ok());
    fails_in_both::<PromptBlock>(r#"{"text":{"text":"hi","later":1}}"#);
}
