//! An `event` before `schema_version` decodes by the same rules as one after it, at the nesting
//! limits too: each case in both key orders, in JSON and in `MessagePack` (read with
//! `MAX_MSGPACK_DEPTH`, as every reader does), where the format can write it. Past the format's
//! own nesting limit the held copy fails; in place, `serde_json` skips a newer event or an
//! unknown field at any depth.

use serde::de::DeserializeOwned;

use crate::{Envelope, EnvelopeRead, MAX_MSGPACK_DEPTH, MAX_RAW_DEPTH, RawPayload, SCHEMA_VERSION};

/// The string `encode` replaces with nested lists.
const MARKER: &str = "@@nested@@";

/// The envelope the cases edit, at the current version.
const BASE: &str = r#"{"schema_version":1,"event_id":"01920000-0000-7000-8000-000000000001",
    "stream":"thread:01920000-0000-7000-8000-000000000002","seq":42,"ts":1760000000000,
    "event":{"turn_started":{"turn":"01920000-0000-7000-8000-000000000003",
    "checkpoint_before":"01920000-0000-7000-8000-000000000004"}}}"#;

/// JSON and `MessagePack`, by index.
const JSON: usize = 0;
const PACKED: usize = 1;

/// The map key `name`.
fn key(name: &str) -> RawPayload {
    RawPayload::Str(name.into())
}

/// A map of `entries`.
fn map(entries: Vec<(RawPayload, RawPayload)>) -> RawPayload {
    RawPayload::Map(entries.into())
}

/// `{kind: payload}`.
fn event_of(kind: &str, payload: RawPayload) -> RawPayload {
    map(vec![(key(kind), payload)])
}

/// The base envelope's map as the format `at` writes it (`MessagePack`'s ids binary), at
/// `version`, its event changed by `edit`, and `event` first if `first`, else last.
fn envelope(
    at: usize,
    version: u64,
    first: bool,
    edit: &dyn Fn(RawPayload) -> RawPayload,
) -> RawPayload {
    let base: Envelope = serde_json::from_str(BASE).unwrap();
    let written = if at == JSON {
        serde_json::from_str(BASE).unwrap()
    } else {
        rmp_serde::from_slice(&rmp_serde::to_vec_named(&base).unwrap()).unwrap()
    };
    let RawPayload::Map(entries) = written else {
        panic!("not a map: {written:?}");
    };
    let mut entries = entries.into_vec();
    let at_event = entries.iter().position(|(name, _)| *name == key("event"));
    let (_, event) = entries.remove(at_event.unwrap());
    let at_version = entries
        .iter()
        .position(|(name, _)| *name == key("schema_version"));
    entries[at_version.unwrap()].1 = RawPayload::U64(version);
    let event = (key("event"), edit(event));
    if first {
        entries.insert(0, event);
    } else {
        entries.push(event);
    }
    map(entries)
}

/// `value` in the format `at`, with the string `MARKER` replaced by `fill` (bytes of that format).
fn encode(at: usize, value: &RawPayload, fill: &[u8]) -> Vec<u8> {
    let (bytes, marker) = if at == JSON {
        let text = serde_json::to_vec(value).unwrap();
        (text, serde_json::to_vec(MARKER).unwrap())
    } else {
        let bytes = rmp_serde::to_vec_named(value).unwrap();
        (bytes, rmp_serde::to_vec(MARKER).unwrap())
    };
    let Some(found) = bytes
        .windows(marker.len())
        .position(|window| window == marker.as_slice())
    else {
        return bytes;
    };
    [&bytes[..found], fill, &bytes[found + marker.len()..]].concat()
}

/// `n` nested lists around a null, in the format `at`.
fn nested(at: usize, n: usize) -> Vec<u8> {
    if at == JSON {
        format!("{}null{}", "[".repeat(n), "]".repeat(n)).into_bytes()
    } else {
        let mut bytes = vec![0x91; n];
        bytes.push(0xc0);
        bytes
    }
}

/// Decodes `bytes` of the format `at` as `T`; errors as text.
fn read<T: DeserializeOwned>(at: usize, bytes: &[u8]) -> Result<T, String> {
    if at == JSON {
        serde_json::from_slice(bytes).map_err(|error| error.to_string())
    } else {
        let mut deserializer = rmp_serde::Deserializer::from_read_ref(bytes);
        deserializer.set_max_depth(MAX_MSGPACK_DEPTH);
        T::deserialize(&mut deserializer).map_err(|error| error.to_string())
    }
}

/// The envelope at `version` whose event `edit` gives, with `MARKER` filled by `fill`, read as
/// an `EnvelopeRead` in order and with `event` first.
fn both_orders(
    at: usize,
    version: u64,
    edit: &dyn Fn(RawPayload) -> RawPayload,
    fill: &[u8],
) -> [Result<EnvelopeRead, String>; 2] {
    [false, true].map(|first| read(at, &encode(at, &envelope(at, version, first, edit), fill)))
}

/// `{"later_kind": MARKER}`, an unknown kind whose payload is the marker.
fn later_kind(_: RawPayload) -> RawPayload {
    event_of("later_kind", key(MARKER))
}

/// The base event with an unknown field `x` holding the marker in `turn_started`'s payload.
fn unknown_field(event: RawPayload) -> RawPayload {
    let RawPayload::Map(outer) = event else {
        panic!("not a map: {event:?}");
    };
    let [(kind, RawPayload::Map(fields))] = &*outer else {
        panic!("not one kind: {outer:?}");
    };
    let mut fields = fields.to_vec();
    fields.push((key("x"), key(MARKER)));
    map(vec![(kind.clone(), map(fields))])
}

/// The current envelope, as `BASE` decodes.
fn current() -> EnvelopeRead {
    EnvelopeRead::Current(serde_json::from_str(BASE).unwrap())
}

#[test]
fn a_newer_event_reads_as_newer_whatever_it_holds_in_both_orders() {
    let newer = Ok(EnvelopeRead::Newer {
        schema_version: SCHEMA_VERSION + 1,
    });
    let version = u64::from(SCHEMA_VERSION + 1);
    // 99 lists under `{"later_kind":{"a": ...}}`, past `MAX_RAW_DEPTH` counted from the event.
    let deep = |_: RawPayload| event_of("later_kind", event_of("a", key(MARKER)));
    for at in [JSON, PACKED] {
        let read = both_orders(at, version, &deep, &nested(at, 99));
        assert_eq!(read, [newer.clone(), newer.clone()], "format {at}");
    }
    // A list as a map key, which only `MessagePack` can write.
    let list_key = |_: RawPayload| {
        let payload = map(vec![(
            RawPayload::Seq([RawPayload::U64(1)].into()),
            key("v"),
        )]);
        event_of("later_kind", payload)
    };
    let read = both_orders(PACKED, version, &list_key, &[]);
    assert_eq!(read, [newer.clone(), newer], "{read:?}");
    // At this version and older, `RawPayload`'s map-key rule refuses it in both orders.
    for version in [0, u64::from(SCHEMA_VERSION)] {
        for result in both_orders(PACKED, version, &list_key, &[]) {
            let error = result.unwrap_err();
            assert!(
                error.contains("a list or a map as a map key"),
                "{version}: {error}"
            );
        }
    }
}

#[test]
fn an_unknown_kind_counts_max_raw_depth_from_its_own_root_in_both_orders() {
    for at in [JSON, PACKED] {
        let [in_order, first] = both_orders(at, 1, &later_kind, &nested(at, MAX_RAW_DEPTH));
        let Ok(EnvelopeRead::Current(read)) = &in_order else {
            panic!("format {at}: {in_order:?}");
        };
        assert_eq!(read.event.kind(), "later_kind");
        assert_eq!(first, in_order, "format {at}");
        // One more list is past the limit in both orders.
        for result in both_orders(at, 1, &later_kind, &nested(at, MAX_RAW_DEPTH + 1)) {
            let error = result.unwrap_err();
            assert!(
                error.contains("nest deeper than 100 levels"),
                "format {at}: {error}"
            );
        }
    }
}

#[test]
fn an_older_event_gets_raw_payloads_depth_limit_in_both_orders() {
    // `upcast` takes a `RawPayload`, so the limit applies before it refuses version 0.
    for at in [JSON, PACKED] {
        for result in both_orders(at, 0, &later_kind, &nested(at, MAX_RAW_DEPTH - 1)) {
            let error = result.unwrap_err();
            assert!(
                error.contains("no upcaster reads schema_version 0"),
                "{at}: {error}"
            );
        }
        for result in both_orders(at, 0, &later_kind, &nested(at, MAX_RAW_DEPTH)) {
            let error = result.unwrap_err();
            assert!(
                error.contains("nest deeper than 100 levels"),
                "format {at}: {error}"
            );
        }
    }
}

#[test]
fn an_unknown_field_nested_99_deep_is_skipped_in_both_orders() {
    for at in [JSON, PACKED] {
        let read = both_orders(at, 1, &unknown_field, &nested(at, 99));
        assert_eq!(read, [Ok(current()), Ok(current())], "format {at}");
    }
}

#[test]
fn a_held_event_stops_at_the_formats_nesting_limit() {
    // `MessagePack`'s reader counts every value against `MAX_MSGPACK_DEPTH`, kept or skipped.
    let newer = u64::from(SCHEMA_VERSION + 1);
    let deep = nested(PACKED, MAX_MSGPACK_DEPTH);
    for (version, edit) in [
        (newer, &later_kind as &dyn Fn(RawPayload) -> RawPayload),
        (1, &unknown_field),
    ] {
        for result in both_orders(PACKED, version, edit, &deep) {
            let error = result.unwrap_err();
            assert!(error.contains("depth limit exceeded"), "{version}: {error}");
        }
    }
    // A kept payload stops at `MAX_RAW_DEPTH` in order, before the format's limit, which the
    // held copy reaches: `serde_json` holds at most 128 levels.
    for (at, says) in [
        (JSON, "recursion limit exceeded"),
        (PACKED, "depth limit exceeded"),
    ] {
        let [in_order, first] = both_orders(at, 1, &later_kind, &nested(at, 200));
        let error = in_order.unwrap_err();
        assert!(
            error.contains("nest deeper than 100 levels"),
            "{at}: {error}"
        );
        let error = first.unwrap_err();
        assert!(error.contains(says), "{at}: {error}");
    }
    // It skips a newer event and an unknown field in place without counting their depth, so
    // only the held copy reaches its limit there.
    for (version, edit, expected) in [
        (
            newer,
            &later_kind as &dyn Fn(RawPayload) -> RawPayload,
            EnvelopeRead::Newer {
                schema_version: SCHEMA_VERSION + 1,
            },
        ),
        (1, &unknown_field, current()),
    ] {
        let [in_order, first] = both_orders(JSON, version, edit, &nested(JSON, 200));
        assert_eq!(in_order, Ok(expected));
        assert!(first.unwrap_err().contains("recursion limit exceeded"));
    }
}

#[test]
fn a_message_pack_ext_value_in_an_unknown_kind_reads_the_same_in_both_orders() {
    // `d4 05 01`: a fixext1 of type 5 holding the byte 1.
    let [in_order, first] = both_orders(PACKED, 1, &later_kind, &[0xd4, 0x05, 0x01]);
    let Ok(EnvelopeRead::Current(read)) = &in_order else {
        panic!("{in_order:?}");
    };
    let crate::DomainEvent::Unknown { payload, .. } = &read.event else {
        panic!("not unknown: {:?}", read.event);
    };
    let ext = RawPayload::Seq([RawPayload::U64(5), RawPayload::Bytes([1].into())].into());
    assert_eq!(payload, &ext);
    assert_eq!(first, in_order);
}
