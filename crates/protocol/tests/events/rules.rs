//! Rules the encoder and decoder enforce beyond the shape: a decoded `Unknown` writes back
//! under its own kind, the envelope's version rules (`schema_version` picks how `event` is read,
//! in either key order; a newer one reads as `EnvelopeRead::Newer`, an older one goes through
//! the upcaster), each kind's stream as 0002 records it (`DomainEvent::is_matter_level`, `None`
//! for an unknown kind), and the conversions into `DomainEvent`. Nothing here checks an
//! envelope's `stream` against its event: the host does that before it commits. The number
//! rules are in `numbers.rs`.

use std::fmt::Debug;

use clauseo_protocol::{
    DomainEvent, Envelope, EnvelopeRead, RawPayload, SCHEMA_VERSION, SteerOutcome, TextOrBlob,
    TurnStarted,
};
use serde::de::DeserializeOwned;

use crate::examples::{every_event, tool_call_started};
use crate::fixtures::{
    blob, checkpoint_id, entry, envelope, packed_raw, raw, remove_entry, turn_id, unknown_event,
    unknown_of,
};
use crate::markdown;

#[test]
fn a_decoded_unknown_writes_back_as_its_kind() {
    // No other crate can build an `Unknown` under a known kind: only decoding builds a catch-all,
    // from a kind this version does not know. Writing one under a known kind is refused, which
    // the crate's own unit tests check (`open_enum::tests`).
    let later = unknown_of::<SteerOutcome>("later");
    assert_eq!(serde_json::to_string(&later).unwrap(), r#""later""#);
    let event = unknown_event("later_kind", raw("{}"));
    assert_eq!(
        serde_json::to_string(&event).unwrap(),
        r#"{"later_kind":{}}"#
    );
}

/// The `turn_started` envelope these tests edit.
fn turn_started_envelope() -> Envelope {
    envelope(
        TurnStarted {
            turn: turn_id(3),
            checkpoint_before: checkpoint_id(8),
        }
        .into(),
    )
}

/// `current` as JSON and as `MessagePack` (whose ids are binary) write it, each read back as a
/// `RawPayload` and changed by `edit`; [`decode`] writes each back in its own format.
fn edited(current: &Envelope, edit: impl Fn(&mut RawPayload)) -> [RawPayload; 2] {
    let mut forms = [
        raw(&serde_json::to_string(current).unwrap()),
        packed_raw(current),
    ];
    forms.iter_mut().for_each(edit);
    forms
}

/// Writes `value` as JSON (`at` 0) or `MessagePack` (`at` 1) and decodes it as `T`.
fn decode<T: DeserializeOwned>(at: usize, value: &RawPayload) -> Result<T, String> {
    if at == 0 {
        let json = serde_json::to_string(value).unwrap();
        serde_json::from_str(&json).map_err(|error| error.to_string())
    } else {
        let bytes = rmp_serde::to_vec_named(value).unwrap();
        rmp_serde::from_slice(&bytes).map_err(|error| error.to_string())
    }
}

/// Checks that both forms decode to `expected`.
fn decodes_to<T: DeserializeOwned + PartialEq + Debug>(forms: &[RawPayload; 2], expected: &T) {
    for (at, value) in forms.iter().enumerate() {
        assert_eq!(
            decode::<T>(at, value).as_ref(),
            Ok(expected),
            "form {at}: {value:?}"
        );
    }
}

/// Checks that both forms fail to decode as `T` with an error holding `says`.
fn refused_with<T: DeserializeOwned + Debug>(forms: &[RawPayload; 2], says: &str) {
    for (at, value) in forms.iter().enumerate() {
        let error = decode::<T>(at, value).unwrap_err();
        assert!(error.contains(says), "form {at}: {error}");
    }
}

/// Runs `edit` on the entries of the map `map`.
fn edit_entries(map: &mut RawPayload, edit: impl FnOnce(&mut Vec<(RawPayload, RawPayload)>)) {
    let RawPayload::Map(entries) = map else {
        panic!("not a map: {map:?}");
    };
    let mut list = entries.to_vec();
    edit(&mut list);
    *entries = list.into();
}

/// The map key `name`.
fn key(name: &str) -> RawPayload {
    RawPayload::Str(name.into())
}

/// Puts the map's entries in a new order: `order` lists the old positions. An envelope's order is
/// `schema_version`, `event_id`, `stream`, `seq`, `ts`, `command_id`, `event`.
fn reorder(map: &mut RawPayload, order: &'static [usize]) {
    edit_entries(map, |entries| {
        let moved: Vec<_> = order.iter().map(|&i| entries[i].clone()).collect();
        assert_eq!(moved.len(), entries.len());
        *entries = moved;
    });
}

#[test]
fn envelope_at_this_version_decodes_as_current() {
    let current = turn_started_envelope();
    let forms = edited(&current, |_| {});
    decodes_to(&forms, &EnvelopeRead::Current(current.clone()));
    decodes_to(&forms, &current);
    // Other fields may come before `schema_version` (`event` too: see below).
    decodes_to(
        &edited(&current, |map| reorder(map, &[3, 1, 0, 2, 4, 5, 6])),
        &current,
    );
    // A field this version does not know is skipped.
    let extra = edited(&current, |map| {
        edit_entries(map, |entries| {
            entries.insert(4, (key("later"), raw(r#"{"x":[1]}"#)));
        });
    });
    decodes_to(&extra, &current);
    // A null `command_id` is none.
    let mut quiet = current.clone();
    quiet.command_id = None;
    decodes_to(
        &edited(&current, |map| *entry(map, "command_id") = RawPayload::Null),
        &quiet,
    );
}

#[test]
fn envelope_newer_than_this_version_reads_as_newer() {
    let current = turn_started_envelope();
    // A newer Clauseo renamed a field of `turn_started`.
    let rename = |map: &mut RawPayload| {
        edit_entries(entry(entry(map, "event"), "turn_started"), |entries| {
            for (name, _) in entries.iter_mut() {
                if *name == key("checkpoint_before") {
                    *name = key("checkpoint");
                }
            }
        });
    };
    // This version cannot read the payload ...
    refused_with::<EnvelopeRead>(
        &edited(&current, rename),
        "missing field `checkpoint_before`",
    );
    // ... and still reads the envelope as newer, not as a bad one.
    let next = SCHEMA_VERSION + 1;
    let newer = edited(&current, |map| {
        rename(map);
        *entry(map, "schema_version") = RawPayload::U64(next.into());
    });
    let read = EnvelopeRead::Newer {
        schema_version: next,
    };
    decodes_to(&newer, &read);
    // The rest of the map is consumed, so the next value in a list still reads.
    let plain = edited(&current, |_| {});
    let lists = [0, 1].map(|at| RawPayload::Seq([newer[at].clone(), plain[at].clone()].into()));
    decodes_to(&lists, &vec![read, EnvelopeRead::Current(current)]);
    // As a plain `Envelope` it is an error that says what to decode instead.
    let says =
        format!("schema_version {next} is newer than {SCHEMA_VERSION}; decode an EnvelopeRead");
    refused_with::<Envelope>(&newer, &says);
}

/// Moves the map's `event` entry to the front, before `schema_version`.
fn event_first(map: &mut RawPayload) {
    edit_entries(map, |entries| {
        let at = entries.iter().position(|(name, _)| *name == key("event"));
        let moved = entries.remove(at.unwrap());
        entries.insert(0, moved);
    });
}

/// A `tool_call_started` whose raw input and output are blob refs, so its `BlobHash`es are
/// `MessagePack` binary data; every example event holds its text inline.
fn tool_call_started_with_blobs() -> DomainEvent {
    let mut started = tool_call_started();
    started.raw_input = Some(TextOrBlob::Blob(blob(7, 4096)));
    started.raw_output = Some(TextOrBlob::Blob(blob(9, 8192)));
    started.into()
}

#[test]
fn envelope_with_its_event_before_schema_version_decodes_the_same() {
    let mut events = every_event();
    events.push(tool_call_started_with_blobs());
    for event in events {
        let current = envelope(event);
        // A `serde_json::Value` sorts its keys, so `event` comes before `schema_version`.
        let sorted = serde_json::to_value(&current).unwrap();
        let keys: Vec<&String> = sorted.as_object().unwrap().keys().collect();
        let at = |name: &str| keys.iter().position(|key| *key == name).unwrap();
        assert!(at("event") < at("schema_version"), "{keys:?}");
        // It sorts a `RawPayload` map's keys too, so the check is that the decode writes back
        // as the same `Value` (whose maps compare without order).
        let read = serde_json::from_value::<Envelope>(sorted.clone()).unwrap();
        assert_eq!(serde_json::to_value(&read).unwrap(), sorted);
        assert_eq!(
            serde_json::from_value::<EnvelopeRead>(sorted).unwrap(),
            EnvelopeRead::Current(read)
        );
        // In both formats, `MessagePack`'s binary ids and blob hashes included.
        let moved = edited(&current, event_first);
        decodes_to(&moved, &EnvelopeRead::Current(current.clone()));
        decodes_to(&moved, &current);
    }
}

#[test]
fn envelope_reads_an_event_before_schema_version_by_the_version() {
    let current = turn_started_envelope();
    // A payload this version cannot read fails as it does in place ...
    let rename = |map: &mut RawPayload| {
        let payload = entry(entry(map, "event"), "turn_started");
        edit_entries(payload, |entries| {
            entries.retain(|(name, _)| *name != key("turn"));
        });
        event_first(map);
    };
    refused_with::<EnvelopeRead>(&edited(&current, rename), "missing field `turn`");
    // ... and a newer version still reads as newer, or as an error for a plain `Envelope`.
    let next = SCHEMA_VERSION + 1;
    let newer = edited(&current, |map| {
        rename(map);
        *entry(map, "schema_version") = RawPayload::U64(next.into());
    });
    decodes_to(
        &newer,
        &EnvelopeRead::Newer {
            schema_version: next,
        },
    );
    refused_with::<Envelope>(&newer, &format!("schema_version {next} is newer"));
    // An older one goes through the upcaster.
    let older = edited(&current, |map| {
        *entry(map, "schema_version") = RawPayload::U64(0);
        event_first(map);
    });
    refused_with::<EnvelopeRead>(&older, "Envelope: no upcaster reads schema_version 0");
    // Without a version the event is never decoded.
    let no_version = edited(&current, |map| {
        event_first(map);
        remove_entry(map, "schema_version");
    });
    refused_with::<EnvelopeRead>(&no_version, "missing field `schema_version`");
    refused_with::<Envelope>(&no_version, "missing field `schema_version`");
    // An `event` read early and again, before or after the version, is a repeat.
    for at in [1, 2] {
        let twice = edited(&current, |map| {
            event_first(map);
            edit_entries(map, |entries| entries.insert(at, entries[0].clone()));
        });
        refused_with::<EnvelopeRead>(&twice, "duplicate field `event`");
    }
}

#[test]
fn envelope_refuses_the_message_pack_array_form() {
    // `rmp_serde::to_vec` writes a struct as an array by position; an envelope is a map only.
    let current = turn_started_envelope();
    let bytes = rmp_serde::to_vec(&current).unwrap();
    let says = "invalid type: sequence, expected an envelope";
    let error = rmp_serde::from_slice::<EnvelopeRead>(&bytes).unwrap_err();
    assert!(error.to_string().contains(says), "{error}");
    let error = rmp_serde::from_slice::<Envelope>(&bytes).unwrap_err();
    assert!(error.to_string().contains(says), "{error}");
}

#[test]
fn envelope_older_than_any_upcaster_is_refused() {
    let current = turn_started_envelope();
    let older = edited(&current, |map| {
        *entry(map, "schema_version") = RawPayload::U64(0);
    });
    refused_with::<EnvelopeRead>(&older, "Envelope: no upcaster reads schema_version 0");
}

#[test]
fn envelope_refuses_a_missing_or_repeated_field() {
    let current = turn_started_envelope();
    for field in ["event_id", "stream", "seq", "ts", "event"] {
        let missing = edited(&current, |map| remove_entry(map, field));
        refused_with::<EnvelopeRead>(&missing, &format!("missing field `{field}`"));
        refused_with::<Envelope>(&missing, &format!("missing field `{field}`"));
    }
    for (field, at) in [("schema_version", 0), ("seq", 3)] {
        let twice = edited(&current, |map| {
            edit_entries(map, |entries| entries.insert(at, entries[at].clone()));
        });
        refused_with::<EnvelopeRead>(&twice, &format!("duplicate field `{field}`"));
        refused_with::<Envelope>(&twice, &format!("duplicate field `{field}`"));
    }
    let says = "expected an envelope: a map of its fields";
    refused_with::<EnvelopeRead>(&[RawPayload::U64(5), RawPayload::U64(5)], says);
    refused_with::<Envelope>(&[RawPayload::U64(5), RawPayload::U64(5)], says);
    // A key that is not a string (`MessagePack` allows one) is refused, not skipped.
    let numbered = edited(&current, |map| {
        edit_entries(map, |entries| {
            entries.insert(1, (RawPayload::U64(9), RawPayload::U64(1)));
        });
    });
    let says = "expected an envelope field name";
    for result in [
        decode::<EnvelopeRead>(1, &numbered[1]).map(|_| ()),
        decode::<Envelope>(1, &numbered[1]).map(|_| ()),
    ] {
        assert!(result.as_ref().unwrap_err().contains(says), "{result:?}");
    }
}
/// The event schema record, read at build time.
const RECORD: &str = include_str!("../../../../docs/decisions/0002-event-schema.md");

/// The logged rows of the record's kind table (`### Kinds`): each kind with its Stream cell. A
/// Logged cell must say `yes` or `no`, and a logged row's Kind cell must be one code span.
fn kind_rows(record: &str) -> Vec<(String, String)> {
    let table = markdown::table(record, "Kinds", "Kind");
    let (kind, logged, stream) = (
        table.column("Kind"),
        table.column("Logged"),
        table.column("Stream"),
    );
    let mut rows = Vec::new();
    for row in &table.rows {
        let cell = &row[kind];
        match row[logged].text.as_str() {
            "yes" => match cell.code.as_slice() {
                [one] if cell.text == format!("`{one}`") => {
                    rows.push((one.clone(), row[stream].text.clone()));
                }
                _ => panic!("a logged Kind cell is one code span: {:?}", cell.text),
            },
            "no" => {}
            other => panic!("{}: Logged says {other:?}, neither yes nor no", cell.text),
        }
    }
    rows
}

#[test]
fn kind_table_matches_record() {
    let rows = kind_rows(RECORD);
    let listed: Vec<&str> = rows.iter().map(|(kind, _)| kind.as_str()).collect();
    assert_eq!(
        listed,
        DomainEvent::KINDS,
        "0002's logged kinds, in table order"
    );
    let events = every_event();
    for (kind, stream) in &rows {
        let event = events
            .iter()
            .find(|event| event.kind() == kind)
            .unwrap_or_else(|| panic!("no example of {kind}"));
        let expected = match stream.as_str() {
            "M" => true,
            "T" => false,
            other => panic!("{kind}: Stream {other:?} is neither T nor M"),
        };
        assert_eq!(
            event.is_matter_level(),
            Some(expected),
            "{kind}: 0002 says stream {stream}, is_matter_level says otherwise"
        );
    }
    // A kind this version does not know has no stream here, whatever its name says.
    let unknown = unknown_event("later_matter_kind", raw("{}"));
    assert_eq!(unknown.is_matter_level(), None);
}

#[test]
fn boxed_variants_convert_from_the_bare_payload() {
    let started = tool_call_started();
    assert_eq!(
        DomainEvent::from(started.clone()),
        DomainEvent::ToolCallStarted(Box::new(started.clone()))
    );
    assert_eq!(
        DomainEvent::from(Box::new(started.clone())),
        DomainEvent::from(started)
    );
    for event in every_event() {
        if let DomainEvent::ContentAppended(boxed) = &event {
            assert_eq!(DomainEvent::from((**boxed).clone()), event);
        }
        if let DomainEvent::ToolCallUpdated(boxed) = &event {
            assert_eq!(DomainEvent::from((**boxed).clone()), event);
        }
    }
}

#[test]
fn a_wrong_type_names_a_domain_event() {
    let error = serde_json::from_str::<DomainEvent>("5")
        .unwrap_err()
        .to_string();
    let says = "expected a domain event: a map with one key, the kind";
    assert!(error.contains(says), "{error}");
}
