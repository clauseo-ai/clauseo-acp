//! Unit tests for the open-enum machinery, on a private enum of each shape, in JSON and in
//! `MessagePack`.

use serde::de::MapAccess;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::{OpenDataEnum, deserialize_open, serialize_one_key};
use crate::RawPayload;

super::open_unit_enum! {
    /// A test enum.
    enum Shade {
        /// Plain red.
        Red,
        /// Two words, to check the snake case.
        LightBlue,
    }
}

/// A test enum with data, written the way A2 writes `DetachReason`.
#[derive(Debug, Clone, PartialEq)]
enum Sample {
    Plain,
    Payload { n: u32, label: String },
    Unknown { kind: String },
}

#[derive(Serialize, Deserialize)]
struct PayloadData<L> {
    n: u32,
    label: L,
}

impl Serialize for Sample {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Plain => serializer.serialize_str("plain"),
            Self::Payload { n, label } => serialize_one_key(
                serializer,
                "payload",
                &PayloadData {
                    n: *n,
                    label: label.as_str(),
                },
            ),
            Self::Unknown { kind } => serializer.serialize_str(kind),
        }
    }
}

impl OpenDataEnum for Sample {
    const EXPECTING: &'static str = "a sample";
    const DATA_KINDS: &'static [&'static str] = &["payload"];

    fn unit(kind: &str) -> Option<Self> {
        match kind {
            "plain" => Some(Self::Plain),
            _ => None,
        }
    }

    fn data<'de, A: MapAccess<'de>>(kind: &str, map: &mut A) -> Result<Option<Self>, A::Error> {
        Ok(match kind {
            "payload" => {
                let data: PayloadData<String> = map.next_value()?;
                Some(Self::Payload {
                    n: data.n,
                    label: data.label,
                })
            }
            _ => None,
        })
    }

    fn unknown(kind: &str) -> Self {
        Self::Unknown {
            kind: kind.to_owned(),
        }
    }
}

impl<'de> Deserialize<'de> for Sample {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserialize_open(deserializer)
    }
}

/// The `MessagePack` form of the value a JSON text holds, keys in their JSON order.
fn msgpack_of(json: &str) -> Vec<u8> {
    let raw: RawPayload = serde_json::from_str(json).unwrap();
    rmp_serde::to_vec_named(&raw).unwrap()
}

fn from_both(json: &str) -> (Result<Sample, String>, Result<Sample, String>) {
    let from_json = serde_json::from_str::<Sample>(json).map_err(|e| e.to_string());
    let from_msgpack =
        rmp_serde::from_slice::<Sample>(&msgpack_of(json)).map_err(|e| e.to_string());
    (from_json, from_msgpack)
}

fn decodes_in_both(json: &str, expected: &Sample) {
    let (from_json, from_msgpack) = from_both(json);
    assert_eq!(from_json.as_ref(), Ok(expected), "JSON {json}");
    assert_eq!(from_msgpack.as_ref(), Ok(expected), "MessagePack of {json}");
}

fn fails_in_both(json: &str) {
    let (from_json, from_msgpack) = from_both(json);
    assert!(from_json.is_err(), "JSON {json} decoded to {from_json:?}");
    assert!(
        from_msgpack.is_err(),
        "MessagePack of {json} decoded to {from_msgpack:?}"
    );
}

#[test]
fn known_variants_round_trip() {
    let payload = Sample::Payload {
        n: 7,
        label: "a".to_owned(),
    };
    for (value, json) in [
        (Sample::Plain, r#""plain""#),
        (payload, r#"{"payload":{"n":7,"label":"a"}}"#),
    ] {
        assert_eq!(serde_json::to_string(&value).unwrap(), json);
        decodes_in_both(json, &value);
        let bytes = rmp_serde::to_vec_named(&value).unwrap();
        assert_eq!(bytes, msgpack_of(json));
        assert_eq!(rmp_serde::from_slice::<Sample>(&bytes).unwrap(), value);
    }
}

#[test]
fn unknown_unit_string_is_unknown_and_writes_back() {
    let unknown = Sample::Unknown {
        kind: "later".to_owned(),
    };
    decodes_in_both(r#""later""#, &unknown);
    assert_eq!(serde_json::to_string(&unknown).unwrap(), r#""later""#);
}

#[test]
fn unknown_key_with_map_payload_is_unknown() {
    let json = r#"{"later":{"b":[1,{"c":null}],"a":"x"}}"#;
    decodes_in_both(
        json,
        &Sample::Unknown {
            kind: "later".to_owned(),
        },
    );
}

#[test]
fn unit_variant_as_a_key_with_null_decodes() {
    decodes_in_both(r#"{"plain":null}"#, &Sample::Plain);
}

#[test]
fn escaped_key_decodes_through_the_owned_path() {
    let value = serde_json::from_str::<Sample>(r#"{"pay\u006coad":{"n":1,"label":"b"}}"#).unwrap();
    assert_eq!(
        value,
        Sample::Payload {
            n: 1,
            label: "b".to_owned()
        }
    );
}

#[test]
fn malformed_values_are_errors() {
    fails_in_both(r#"{"payload":{"n":"seven","label":"a"}}"#);
    fails_in_both(r#"{"payload":{"n":1,"label":"a"},"plain":null}"#);
    fails_in_both("{}");
    fails_in_both(r#""payload""#);
    fails_in_both("3");
}

#[test]
fn unit_enum_lists_every_variant_and_names_it() {
    assert_eq!(Shade::ALL, [Shade::Red, Shade::LightBlue, Shade::Unknown]);
    let names: Vec<_> = Shade::ALL.iter().map(Shade::variant_name).collect();
    assert_eq!(names, ["Red", "LightBlue", "Unknown"]);
}

#[test]
fn unit_enum_decodes_unknown_strings_in_both_formats() {
    assert_eq!(
        serde_json::to_string(&Shade::LightBlue).unwrap(),
        r#""light_blue""#
    );
    assert_eq!(
        serde_json::from_str::<Shade>(r#""light_blue""#).unwrap(),
        Shade::LightBlue
    );
    assert_eq!(
        serde_json::from_str::<Shade>(r#""ultraviolet""#).unwrap(),
        Shade::Unknown
    );
    let bytes = rmp_serde::to_vec_named(&Shade::LightBlue).unwrap();
    assert_eq!(bytes, msgpack_of(r#""light_blue""#));
    assert_eq!(
        rmp_serde::from_slice::<Shade>(&bytes).unwrap(),
        Shade::LightBlue
    );
    let later = msgpack_of(r#""ultraviolet""#);
    assert_eq!(
        rmp_serde::from_slice::<Shade>(&later).unwrap(),
        Shade::Unknown
    );
}
