//! Unit tests for the open-enum machinery, on a private enum of each shape, in JSON and in
//! `MessagePack`.

use std::fmt::Debug;

use serde::de::DeserializeOwned;

use super::UnknownKind;
use crate::serde_util::BorrowedStr;
use crate::{DetachReason, DomainEvent, LimitSource, MAX_RAW_DEPTH, RawPayload, SteerOutcome};

super::open_unit_enum! {
    /// A test enum.
    enum Shade {
        /// Plain red.
        Red,
        /// Two words, to check the snake case.
        LightBlue,
    }
    test mod shade;
}

super::open_data_enum! {
    /// A test enum with data, written the way `DetachReason` is.
    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Sample {
        /// A unit kind.
        Plain = "plain",
        /// A data kind.
        Payload = "payload" {
            n: u32,
            #[serde(skip_serializing_if = "String::is_empty", default)]
            label: String,
        },
        _ =>
        /// A kind this version does not know.
        Unknown,
    }
    test mod sample with [
        Sample::Plain,
        Sample::Payload {
            n: 7,
            label: "a".to_owned(),
        },
    ];
}

/// The `MessagePack` form of the value a JSON text holds, keys in their JSON order.
fn msgpack_of(json: &str) -> Vec<u8> {
    let raw: RawPayload = serde_json::from_str(json).unwrap();
    rmp_serde::to_vec_named(&raw).unwrap()
}

fn from_both<T: DeserializeOwned>(json: &str) -> (Result<T, String>, Result<T, String>) {
    let from_json = serde_json::from_str::<T>(json).map_err(|e| e.to_string());
    let from_msgpack = rmp_serde::from_slice::<T>(&msgpack_of(json)).map_err(|e| e.to_string());
    (from_json, from_msgpack)
}

fn decodes_in_both<T: DeserializeOwned + PartialEq + Debug>(json: &str, expected: &T) {
    let (from_json, from_msgpack) = from_both(json);
    assert_eq!(from_json.as_ref(), Ok(expected), "JSON {json}");
    assert_eq!(from_msgpack.as_ref(), Ok(expected), "MessagePack of {json}");
}

fn fails_in_both<T: DeserializeOwned + Debug>(json: &str) {
    let (from_json, from_msgpack) = from_both::<T>(json);
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
        kind: UnknownKind::new("later"),
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
            kind: UnknownKind::new("later"),
        },
    );
}

/// Checks that `json` fails to decode in both formats with an error that says `says`.
fn fails_in_both_saying<T: DeserializeOwned + Debug>(json: &str, says: &str) {
    for error in errors_of::<T>(json) {
        assert!(error.contains(says), "{json}: {error}");
    }
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
    fails_in_both::<Sample>(r#"{"payload":{"n":"seven","label":"a"}}"#);
    fails_in_both::<Sample>(r#"{"payload":{"n":1,"label":"a"},"plain":null}"#);
    fails_in_both::<Sample>("{}");
    fails_in_both::<Sample>(r#""payload""#);
    fails_in_both::<Sample>("3");
}

#[test]
fn variant_names_follow_the_rows_and_unknown_is_last() {
    assert_eq!(Sample::VARIANT_NAMES, ["Plain", "Payload", "Unknown"]);
}

#[test]
fn field_attributes_apply_to_the_decoder() {
    // `label` is left out when empty and defaults when missing, as its attribute says.
    let value = Sample::Payload {
        n: 2,
        label: String::new(),
    };
    assert_eq!(
        serde_json::to_string(&value).unwrap(),
        r#"{"payload":{"n":2}}"#
    );
    decodes_in_both(r#"{"payload":{"n":2}}"#, &value);
}

/// Checks that `value` fails to encode in both formats with an error naming `kind`.
fn not_written<T: serde::Serialize + Debug>(value: &T, kind: &str) {
    let error = serde_json::to_string(value).unwrap_err().to_string();
    assert!(error.contains(&format!("{kind:?}")), "{error}");
    let error = rmp_serde::to_vec_named(value).unwrap_err().to_string();
    assert!(error.contains(&format!("{kind:?}")), "{error}");
}

#[test]
fn every_catch_all_refuses_a_kind_its_enum_knows() {
    for kind in DomainEvent::KINDS {
        let unknown = DomainEvent::Unknown {
            kind: UnknownKind::new(kind),
            payload: RawPayload::Map(Vec::new().into()),
        };
        not_written(&unknown, kind);
    }
    for kind in ["exited", "closed", "restarting", "idle_stopped"] {
        let unknown = DetachReason::Unknown {
            kind: UnknownKind::new(kind),
        };
        not_written(&unknown, kind);
    }
    let kinds = [
        "fresh",
        "stale",
        "loading",
        "unavailable",
        "not_applicable",
        "live_only",
    ];
    for kind in kinds {
        let unknown = LimitSource::Unknown {
            kind: UnknownKind::new(kind),
        };
        not_written(&unknown, kind);
    }
    for kind in ["injected", "requeued", "sent_now"] {
        let unknown = SteerOutcome::Unknown {
            kind: UnknownKind::new(kind),
        };
        not_written(&unknown, kind);
    }
    // A kind its own enum does not know is written, even when another enum knows it.
    let later = SteerOutcome::Unknown {
        kind: UnknownKind::new("closed"),
    };
    assert_eq!(serde_json::to_string(&later).unwrap(), r#""closed""#);
}

#[test]
fn an_unknown_kind_reads_as_its_text() {
    let kind = UnknownKind::<Sample>::new("later");
    assert_eq!(kind.as_str(), "later");
    assert_eq!(&*kind, "later");
    assert_eq!(kind, *"later");
    assert_ne!(kind, *"other");
    assert_eq!(kind.clone(), kind);
    assert_ne!(kind, UnknownKind::new("other"));
    assert_eq!(format!("{kind:?}"), r#""later""#);
    let hash = |value: &UnknownKind<Sample>| {
        let mut hasher = std::hash::DefaultHasher::new();
        std::hash::Hash::hash(value, &mut hasher);
        std::hash::Hasher::finish(&hasher)
    };
    let mut hasher = std::hash::DefaultHasher::new();
    std::hash::Hash::hash("later", &mut hasher);
    assert_eq!(hash(&kind), std::hash::Hasher::finish(&hasher));
}

#[test]
fn unknown_under_a_known_kind_is_not_written() {
    for kind in ["plain", "payload"] {
        let value = Sample::Unknown {
            kind: UnknownKind::new(kind),
        };
        assert!(serde_json::to_string(&value).is_err(), "{kind}");
    }
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
/// `{key: {"a": {"a": ..}}}` with `depth` maps under `key`, one inside the other, decoded from
/// JSON and from `MessagePack` (written by hand: a `RawPayload` refuses to hold it).
fn deep_in_both<T: DeserializeOwned>(key: &str, depth: usize) -> [Result<T, String>; 2] {
    let opened = r#"{"a":"#.repeat(depth - 1);
    let json = format!(r#"{{"{key}":{opened}{{}}{}}}"#, "}".repeat(depth - 1));
    let mut packed = vec![0x81, 0xa0 | u8::try_from(key.len()).unwrap()];
    packed.extend(key.bytes());
    for _ in 1..depth {
        packed.extend([0x81, 0xa1, b'a']);
    }
    packed.push(0x80);
    [
        serde_json::from_str(&json).map_err(|e| e.to_string()),
        rmp_serde::from_slice(&packed).map_err(|e| e.to_string()),
    ]
}

/// A payload skipped under `key` nests at most `MAX_RAW_DEPTH` maps, in both formats.
fn skipped_payload_depth_is_counted<T: DeserializeOwned + PartialEq + Debug>(key: &str, at: &T) {
    for decoded in deep_in_both::<T>(key, MAX_RAW_DEPTH) {
        assert_eq!(decoded.as_ref(), Ok(at));
    }
    for decoded in deep_in_both::<T>(key, MAX_RAW_DEPTH + 1) {
        assert!(decoded.is_err(), "{decoded:?}");
    }
}

/// Under the known unit kind `key`, every payload is refused in both formats, naming the kind:
/// a map (empty or not), `null`, a number, a string, a list, a bool and (`MessagePack` only)
/// bytes.
fn a_unit_kind_takes_no_payload<T: DeserializeOwned + Debug>(key: &str) {
    let says = format!("`{key}` has no fields, so it is a plain string, not a map");
    for payload in [
        r#"{"device":"x","more":[1,{"a":null}]}"#,
        "{}",
        "null",
        "5",
        r#""x""#,
        "[1,2]",
        "true",
    ] {
        fails_in_both_saying::<T>(&format!(r#"{{"{key}":{payload}}}"#), &says);
    }
    let mut packed = vec![0x81, 0xa0 | u8::try_from(key.len()).unwrap()];
    packed.extend(key.bytes());
    packed.extend([0xc4, 0x01, 0x07]);
    let decoded = rmp_serde::from_slice::<T>(&packed).map_err(|e| e.to_string());
    assert!(
        decoded.as_ref().is_err_and(|error| error.contains(&says)),
        "bytes under {key}: {decoded:?}"
    );
}

#[test]
fn a_unit_variant_as_a_key_is_refused_whatever_its_payload() {
    a_unit_kind_takes_no_payload::<Sample>("plain");
}

#[test]
fn a_struct_variant_as_a_plain_string_is_refused() {
    let says = "`payload` has fields, so it is a one-key map, not a plain string";
    fails_in_both_saying::<Sample>(r#""payload""#, says);
    fails_in_both::<Sample>(r#"{"payload":null}"#);
}

#[test]
fn an_unknown_key_takes_any_payload_with_its_depth_counted() {
    let unknown = Sample::Unknown {
        kind: UnknownKind::new("zz"),
    };
    for payload in ["5", r#""x""#, "[1,[2]]", "true", "null", r#"{"nm":380}"#] {
        decodes_in_both(&format!(r#"{{"zz":{payload}}}"#), &unknown);
    }
    skipped_payload_depth_is_counted("zz", &unknown);
}

#[test]
fn unit_enum_skips_any_payload_under_an_unknown_kind() {
    for payload in ["5", r#""x""#, "[1,[2]]", "true", "null", r#"{"nm":380}"#] {
        decodes_in_both(&format!(r#"{{"zz":{payload}}}"#), &Shade::Unknown);
    }
    let packed = [0x81, 0xa2, b'z', b'z', 0xc4, 0x01, 0x07];
    let decoded = rmp_serde::from_slice::<Shade>(&packed).map_err(|e| e.to_string());
    assert_eq!(decoded, Ok(Shade::Unknown), "bytes under an unknown kind");
    skipped_payload_depth_is_counted("zz", &Shade::Unknown);
    fails_in_both::<Shade>(r#"{"zz":5,"red":null}"#);
}

#[test]
fn unit_enum_refuses_a_known_value_as_a_key() {
    a_unit_kind_takes_no_payload::<Shade>("red");
    a_unit_kind_takes_no_payload::<Shade>("light_blue");
    decodes_in_both(r#"{"ultraviolet":{"nm":380}}"#, &Shade::Unknown);
    decodes_in_both(r#"{"zz":null}"#, &Shade::Unknown);
    for bad in ["{}", r#"{"zz":null,"yy":null}"#, "null", "[]", "3"] {
        fails_in_both::<Shade>(bad);
    }
}

/// The error each format gives for `json`, in JSON then `MessagePack`.
fn errors_of<T: DeserializeOwned + Debug>(json: &str) -> [String; 2] {
    let (from_json, from_msgpack) = from_both::<T>(json);
    [from_json, from_msgpack].map(|decoded| decoded.unwrap_err())
}

#[test]
fn a_wrong_type_names_what_each_visitor_expects() {
    for error in errors_of::<Sample>("5") {
        assert!(error.contains("expected enum Sample"), "{error}");
    }
    for error in errors_of::<Shade>("5") {
        assert!(error.contains("expected enum Shade"), "{error}");
    }
    for error in errors_of::<Sample>(r#"{"payload":5}"#) {
        assert!(error.contains("expected struct variant"), "{error}");
    }
    let Err(error) = serde_json::from_str::<BorrowedStr<'_>>("5") else {
        panic!("a number decoded as a string");
    };
    assert!(error.to_string().contains("expected a string"), "{error}");
}
