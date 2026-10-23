//! What only this crate can check: the getter reads `schema_version`, which no other crate can
//! set (the field is private, so every envelope outside holds `SCHEMA_VERSION`), and the
//! deserializer over a held `RawPayload` reads an enum as JSON does, where no logged type reaches,
//! and the seed that holds an early event names what it expects. The key orders' cases are in
//! `order.rs`.

use serde::Deserialize;
use serde::de::DeserializeSeed;
use serde::de::value::{self, I128Deserializer};

use super::Envelope;
use super::held::HeldSeed;
use super::raw_de::RawDeserializer;
use crate::RawPayload;

mod order;

#[test]
fn schema_version_reads_the_field() {
    let json = r#"{"schema_version":1,"event_id":"01920000-0000-7000-8000-000000000001",
        "stream":"thread:01920000-0000-7000-8000-000000000002","seq":42,"ts":1760000000000,
        "event":{"thread_archived":{}}}"#;
    let envelope: Envelope = serde_json::from_str(json).unwrap();
    assert_eq!(envelope.schema_version(), 1);
    let bumped = Envelope {
        schema_version: 7,
        ..envelope
    };
    assert_eq!(bumped.schema_version(), 7);
}
/// An enum with a unit and a newtype variant, as serde derives it.
#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Shape {
    Unit,
    Newtype(u8),
}

/// `json` read as a `RawPayload` and decoded as a `Shape` through `RawDeserializer`, and
/// `json` decoded as a `Shape` in place; errors as text.
fn both(json: &str) -> [Result<Shape, String>; 2] {
    let payload: RawPayload = serde_json::from_str(json).unwrap();
    let held = Shape::deserialize(RawDeserializer::<serde_json::Error>::new(&payload, true));
    let direct = serde_json::from_str::<Shape>(json);
    [held, direct].map(|result| result.map_err(|error| error.to_string()))
}

#[test]
fn a_held_payload_reads_an_enum_as_json_does() {
    for json in [r#""unit""#, r#"{"unit":null}"#, r#"{"newtype":3}"#] {
        let [held, direct] = both(json);
        assert_eq!(held, direct, "{json}");
        assert!(held.is_ok(), "{json}: {held:?}");
    }
    // A unit variant's value must be null; a map with two entries is no variant.
    for json in [r#"{"unit":5}"#, r#"{"unit":null,"newtype":3}"#, "5"] {
        let [held, direct] = both(json);
        assert!(
            held.is_err() && direct.is_err(),
            "{json}: {held:?} {direct:?}"
        );
    }
}

/// Neither format hands a 128-bit integer to the seed, so the error names what it expected.
#[test]
fn the_held_seed_expects_any_value() {
    let error = HeldSeed
        .deserialize(I128Deserializer::<value::Error>::new(1))
        .unwrap_err();
    assert!(error.to_string().contains("expected any value"), "{error}");
}
