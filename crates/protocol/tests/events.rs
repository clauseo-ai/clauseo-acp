//! Events: the envelope and `DomainEvent` on the wire, in JSON and in `MessagePack`, and the
//! enums that decode values a newer Clauseo may add.

#![forbid(clippy::disallowed_methods, clippy::disallowed_types)]

#[cfg(test)]
#[path = "support/fixtures.rs"]
pub mod fixtures;

#[cfg(test)]
#[path = "support/examples.rs"]
pub mod examples;

#[cfg(test)]
#[path = "support/markdown.rs"]
pub mod markdown;

#[cfg(test)]
#[path = "events/unknown_variants.rs"]
mod unknown_variants;

#[cfg(test)]
#[path = "events/open_enums.rs"]
mod open_enums;

#[cfg(test)]
#[path = "events/depth.rs"]
mod depth;

#[cfg(test)]
#[path = "events/rules.rs"]
mod rules;

#[cfg(test)]
#[path = "events/numbers.rs"]
mod numbers;

#[cfg(test)]
#[path = "events/json_copies.rs"]
mod json_copies;

#[cfg(test)]
#[path = "events/constructors.rs"]
mod constructors;

use clauseo_protocol::{
    DomainEvent, ElicitationRequest, ElicitationRequested, Envelope, MAX_RAW_DEPTH,
    MAX_RAW_ENCODE_DEPTH, PromptBlock, RawPayload, SCHEMA_VERSION, StopReason, StreamId,
    ToolCallStarted, TurnEnded, TurnStarted,
};

use examples::{every_event, tool_call_started};
use fixtures::{
    checkpoint_id, decodes_in_both, elicitation_id, entry, envelope, err_contains,
    fails_in_both_with, matter_id, packed_raw, raw, remove_entry, round_trips, thread_id, turn_id,
    unknown_event, v7_text,
};

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
    let bad_streams = [
        ("thread", "expected <kind>:<uuid>"),
        ("topic:x", r#"unknown prefix "topic""#),
        ("thread:not-a-uuid", "not a hyphenated UUID"),
        (nil_v4, "expected a v7 UUID, got version 4"),
    ];
    for (bad, expected) in bad_streams {
        err_contains(bad.parse::<StreamId>(), expected);
        let json = format!("\"{bad}\"");
        err_contains(serde_json::from_str::<StreamId>(&json), expected);
    }
}

#[test]
fn unknown_kind_decodes_to_unknown() {
    let expected = unknown_event(
        "later_kind",
        raw(r#"{"z":1,"a":[true,null,{"y":-2,"b":18446744073709551615}],"m":0.5}"#),
    );
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
    fails_in_both_with::<DomainEvent>("{}", "a map with one key");
    fails_in_both_with::<DomainEvent>(r#"{"a":{},"b":{}}"#, "a map with one key");
    fails_in_both_with::<DomainEvent>(r#""later_kind""#, "a map with one key");
    fails_in_both_with::<DomainEvent>("[]", "a map with one key");
    fails_in_both_with::<DomainEvent>("3", "a map with one key");
}

/// An unknown kind decodes as `Unknown` and writes back the same JSON text, alone and inside an
/// envelope, never under a key named `unknown`. Its one float, 0.5, is exact in binary; other
/// JSON floats come back within one ULP (see `RawPayload`).
#[test]
fn unknown_kind_round_trips_same_value() {
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

/// An unknown kind inside a `MessagePack` envelope decodes to the same value and writes back the
/// same bytes: binary data, an integer key, unsorted keys and a 64-bit float included.
#[test]
fn unknown_kind_round_trips_same_value_in_messagepack() {
    // The stored form `store` writes: a named-map envelope with binary ids, and a payload holding
    // binary data, an integer key and keys in no sorted order.
    let payload = RawPayload::Map(
        vec![
            (
                RawPayload::Str("z".into()),
                RawPayload::Bytes(vec![0, 159, 255].into()),
            ),
            (RawPayload::U64(7), RawPayload::I64(-9)),
            (RawPayload::Str("a".into()), RawPayload::F64(1.5)),
        ]
        .into(),
    );
    let event = unknown_event("later_kind", payload);
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
    assert_eq!(entries[0].0, RawPayload::Str("later_kind".into()));

    // `serde_json::Value` cannot hold the binary payload, which is why `RawPayload` exists.
    let read = rmp_serde::from_slice::<serde_json::Value>(&alone);
    err_contains(read, "invalid type: byte array");
}

/// `depth` lists of one item each around `0`.
#[cfg(test)]
fn nested(depth: usize) -> RawPayload {
    (0..depth).fold(RawPayload::U64(0), |inner, _| {
        RawPayload::Seq(vec![inner].into())
    })
}

/// Checks that `value` fails to encode in JSON and in `MessagePack` with the depth error for
/// `limit`.
#[cfg(test)]
fn too_deep_to_encode<T: serde::Serialize>(value: &T, limit: usize) {
    let errors = [
        serde_json::to_string(value).unwrap_err().to_string(),
        rmp_serde::to_vec_named(value).unwrap_err().to_string(),
    ];
    for error in errors {
        assert!(
            error.contains(&format!("deeper than {limit} levels")),
            "{error}"
        );
    }
}

/// Inside an envelope, in JSON (whose own limit counts the envelope's levels too) and in
/// `MessagePack`: an elicitation schema encodes up to [`MAX_RAW_ENCODE_DEPTH`] and fails one
/// level more, so a producer's too-deep value fails at commit instead of at replay, while it
/// decodes up to [`MAX_RAW_DEPTH`]; an unknown event's payload, which a catch-all decoded,
/// round-trips at [`MAX_RAW_DEPTH`] and fails to encode one level more.
#[test]
fn a_schema_encodes_to_the_encode_limit_and_decodes_to_the_decode_limit() {
    let unknown = |depth| envelope(unknown_event("zz", nested(depth)));
    let elicitation = |depth| {
        envelope(DomainEvent::from(ElicitationRequested {
            elicitation: elicitation_id(12),
            request: ElicitationRequest::Form {
                message: "Which party?".to_owned(),
                schema: nested(depth),
            },
        }))
    };
    round_trips(&unknown(MAX_RAW_DEPTH));
    too_deep_to_encode(&unknown(MAX_RAW_DEPTH + 1), MAX_RAW_DEPTH);
    round_trips(&elicitation(MAX_RAW_ENCODE_DEPTH));
    too_deep_to_encode(&elicitation(MAX_RAW_ENCODE_DEPTH + 1), MAX_RAW_ENCODE_DEPTH);

    // Decoding: the schema written by hand at each depth, in JSON and in `MessagePack`.
    let shallow = elicitation(0);
    let json = serde_json::to_string(&shallow).unwrap();
    let packed = rmp_serde::to_vec_named(&shallow).unwrap();
    for (depth, ok) in [(MAX_RAW_DEPTH, true), (MAX_RAW_DEPTH + 1, false)] {
        let schema = format!("{}0{}", "[".repeat(depth), "]".repeat(depth));
        let deep_json = json.replace(r#""schema":0"#, &format!(r#""schema":{schema}"#));
        let from_json = serde_json::from_str::<Envelope>(&deep_json);
        let at = packed.windows(7).position(|w| w == b"\xa6schema").unwrap() + 7;
        let deep_packed = [&packed[..at], &vec![0x91; depth], &packed[at..]].concat();
        let from_packed = rmp_serde::from_slice::<Envelope>(&deep_packed);
        assert_eq!(from_json.is_ok(), ok, "JSON at {depth}: {from_json:?}");
        assert_eq!(
            from_packed.is_ok(),
            ok,
            "MessagePack at {depth}: {from_packed:?}"
        );
        if ok {
            assert_eq!(from_json.unwrap(), from_packed.unwrap());
        }
    }
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
    // The schema's keys are out of sorted order, so the line above shows the order was kept.
    // (Not shown with `serde_json::Value`: it keeps the order too when the workspace build turns
    // on its `preserve_order` feature.)
    assert!(schema_json.find(r#""type""#) < schema_json.find(r#""required""#));
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
            .any(|(k, _)| *k == RawPayload::Str("command_id".into()))
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
    let decoded: DomainEvent = serde_json::from_str(&good).unwrap();
    assert_eq!(decoded, expected);

    let missing = format!(r#"{{"turn_started":{{"turn":"{}"}}}}"#, v7_text(3));
    let bad_id = r#"{"turn_started":{"turn":"not-a-uuid","checkpoint_before":"x"}}"#;
    let v4 = "00000000-0000-4000-8000-000000000001";
    let lawyer = r#"{"thread_renamed":{"by":"lawyer"}}"#;
    let not_v7 = format!(r#"{{"turn_started":{{"turn":"{v4}","checkpoint_before":"{v4}"}}}}"#);
    for (bad, substring) in [
        (missing.as_str(), "missing field `checkpoint_before`"),
        (bad_id, "not a hyphenated UUID"),
        (not_v7.as_str(), "expected a v7 UUID, got version 4"),
        (r#"{"turn_started":"x"}"#, "expected struct TurnStarted"),
        (r#"{"turn_started":null}"#, "expected struct TurnStarted"),
        (r#"{"thread_renamed":{"title":"t","by":7}}"#, "integer `7`"),
        (lawyer, "missing field `title`"),
    ] {
        err_contains(serde_json::from_str::<DomainEvent>(bad), substring);
    }

    // The same in MessagePack, starting from what `to_vec_named` writes for the good event.
    let corrupt: [fn(&mut RawPayload); 5] = [
        |p| remove_entry(p, "checkpoint_before"),
        |p| *entry(p, "turn") = RawPayload::Bytes(vec![7; 15].into()),
        |p| *entry(p, "turn") = RawPayload::Bytes(uuid::Uuid::nil().as_bytes().to_vec().into()),
        |p| *p = RawPayload::Str("x".into()),
        |p| *p = RawPayload::Null,
    ];
    let substrings = [
        "missing field `checkpoint_before`",
        "invalid length 15, expected 16 bytes",
        "expected a v7 UUID, got version 0",
        "expected struct TurnStarted",
        "invalid type: unit value, expected struct TurnStarted",
    ];
    let bytes = rmp_serde::to_vec_named(&packed_raw(&expected)).unwrap();
    let decoded: DomainEvent = rmp_serde::from_slice(&bytes).unwrap();
    assert_eq!(decoded, expected);
    for (corrupt, substring) in corrupt.into_iter().zip(substrings) {
        let mut packed = packed_raw(&expected);
        corrupt(entry(&mut packed, "turn_started"));
        let bytes = rmp_serde::to_vec_named(&packed).unwrap();
        err_contains(rmp_serde::from_slice::<DomainEvent>(&bytes), substring);
    }
}

#[test]
fn a_struct_payload_also_decodes_from_a_list_by_position() {
    // serde reads a struct from a list in field order, in JSON and in MessagePack alike, so a
    // struct payload is its map or its fields as a list.
    let expected = DomainEvent::from(TurnStarted {
        turn: turn_id(3),
        checkpoint_before: checkpoint_id(8),
    });
    let list = format!(r#"{{"turn_started":["{}","{}"]}}"#, v7_text(3), v7_text(8));
    assert_eq!(
        serde_json::from_str::<DomainEvent>(&list).unwrap(),
        expected
    );
    let short = format!(r#"{{"turn_started":["{}"]}}"#, v7_text(3));
    err_contains(
        serde_json::from_str::<DomainEvent>(&short),
        "invalid length 1",
    );
    // `to_vec` writes the struct as an array, which MessagePack reads back by position.
    let packed = rmp_serde::to_vec(&expected).unwrap();
    assert_eq!(
        rmp_serde::from_slice::<DomainEvent>(&packed).unwrap(),
        expected
    );
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

    for event in &events {
        let json: RawPayload =
            serde_json::from_str(&serde_json::to_string(event).unwrap()).unwrap();
        let RawPayload::Map(entries) = json else {
            panic!("{} is not a map", event.kind());
        };
        assert_eq!(entries.len(), 1, "{}", event.kind());
        assert_eq!(entries[0].0, RawPayload::Str(event.kind().into()));
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
    assert_eq!(written.schema_version(), SCHEMA_VERSION);
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
    let seq_zero = "Seq: 0 is outside 1..=9007199254740991";
    err_contains(serde_json::from_str::<Envelope>(&zero), seq_zero);
    let mut packed = packed_raw(&written);
    *entry(&mut packed, "seq") = RawPayload::U64(0);
    let bytes = rmp_serde::to_vec_named(&packed).unwrap();
    err_contains(rmp_serde::from_slice::<Envelope>(&bytes), seq_zero);
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
        let mut extended = entries.to_vec();
        extended.push((RawPayload::Str("later".into()), raw(r#"{"x":[1]}"#)));
        *entries = extended.into();
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
    let later = r#"{"text":{"text":"hi","later":1}}"#;
    fails_in_both_with::<PromptBlock>(later, "unknown field `later`");
}
