//! The tests [`open_unit_enum!`](super::open_unit_enum) and
//! [`open_data_enum!`](super::open_data_enum) write beside each enum, in JSON and in `MessagePack`
//! (written with `rmp_serde::to_vec_named`, as the stored form is).

use std::fmt::Debug;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::CatchAll;
use crate::RawPayload;

/// A kind no version of Clauseo writes.
const LATER: &str = "zz_later_kind";

/// Input every open enum refuses: no key, two keys, no string or map at all.
const MALFORMED: [&str; 5] = ["{}", r#"{"a":null,"b":null}"#, "3", "[]", "null"];

/// The `MessagePack` form of the value a JSON text holds, keys in their JSON order.
fn msgpack_of(json: &str) -> Vec<u8> {
    let raw: RawPayload = serde_json::from_str(json).unwrap();
    rmp_serde::to_vec_named(&raw).unwrap()
}

/// `json` decoded from JSON and from its `MessagePack` form, named by format.
fn decoded<T: DeserializeOwned>(json: &str) -> [(&'static str, Result<T, String>); 2] {
    [
        (
            "JSON",
            serde_json::from_str(json).map_err(|e| e.to_string()),
        ),
        (
            "MessagePack",
            rmp_serde::from_slice(&msgpack_of(json)).map_err(|e| e.to_string()),
        ),
    ]
}

/// Checks that `json` decodes to `expected` in both formats.
fn decodes_to<T: DeserializeOwned + PartialEq + Debug>(json: &str, expected: &T) {
    for (format, got) in decoded::<T>(json) {
        assert_eq!(got.as_ref(), Ok(expected), "{format} of {json}");
    }
}

/// Checks that `json` fails to decode in both formats.
fn refused<T: DeserializeOwned + Debug>(json: &str) {
    for (format, got) in decoded::<T>(json) {
        assert!(got.is_err(), "{format} of {json} decoded to {got:?}");
    }
}

/// Checks that `json` fails to decode in both formats with an error that says `says`.
fn refused_saying<T: DeserializeOwned + Debug>(json: &str, says: &str) {
    for (format, got) in decoded::<T>(json) {
        match got {
            Err(error) => assert!(error.contains(says), "{format} of {json}: {error}"),
            Ok(value) => panic!("{format} of {json} decoded to {value:?}"),
        }
    }
}

/// Checks that `value`, written in each format, reads back as itself.
fn round_trips<T: Serialize + DeserializeOwned + PartialEq + Debug>(name: &str, value: &T) {
    let json = serde_json::to_string(value).unwrap_or_else(|e| panic!("{name}: {e}"));
    let back: Result<T, _> = serde_json::from_str(&json);
    assert_eq!(
        back.as_ref().ok(),
        Some(value),
        "{name} from JSON {json}: {back:?}"
    );
    let packed = rmp_serde::to_vec_named(value).unwrap_or_else(|e| panic!("{name}: {e}"));
    let back: Result<T, _> = rmp_serde::from_slice(&packed);
    assert_eq!(
        back.as_ref().ok(),
        Some(value),
        "{name} from MessagePack: {back:?}"
    );
}

/// The kind a value is written under: its plain string, or the one key of its map.
fn kind_of<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(kind)) => kind,
        Ok(Value::Object(map)) => map.keys().next().cloned().unwrap_or_default(),
        other => panic!("not a kind: {other:?}"),
    }
}

/// A known kind `value` in the form its variant does not have is refused: a unit kind as a key,
/// whatever its payload, and a struct kind as a plain string or with `null` under its key.
fn the_other_form_is_refused<T>(value: &T)
where
    T: Serialize + DeserializeOwned + Debug,
{
    let kind = kind_of(value);
    let is_unit = matches!(serde_json::to_value(value), Ok(Value::String(_)));
    if is_unit {
        let says = format!("`{kind}` has no fields, so it is a plain string, not a map");
        for payload in [r#"{"later":[1,{"a":null}]}"#, "{}", "null", "5"] {
            refused_saying::<T>(&format!(r#"{{"{kind}":{payload}}}"#), &says);
        }
    } else {
        let says = format!("`{kind}` has fields, so it is a one-key map, not a plain string");
        refused_saying::<T>(&format!(r#""{kind}""#), &says);
        refused::<T>(&format!(r#"{{"{kind}":null}}"#));
    }
}

/// The test of an [`open_unit_enum!`](super::open_unit_enum): `all` is its `ALL`, `unknown` its
/// catch-all, which comes last.
pub(crate) fn unit_enum<T>(all: &[T], unknown: &T, variant_name: fn(&T) -> &'static str)
where
    T: Serialize + DeserializeOwned + PartialEq + Debug,
{
    assert_eq!(all.last(), Some(unknown), "Unknown comes last in ALL");
    for value in all {
        round_trips(variant_name(value), value);
        if value != unknown {
            the_other_form_is_refused(value);
        }
    }
    decodes_to(&format!(r#""{LATER}""#), unknown);
    decodes_to(&format!(r#"{{"{LATER}":{{"a":[1]}}}}"#), unknown);
    decodes_to(&format!(r#"{{"{LATER}":null}}"#), unknown);
    for bad in MALFORMED {
        refused::<T>(bad);
    }
}

/// The test of an [`open_data_enum!`](super::open_data_enum): `samples` hold one value per row,
/// whose Rust names are `rows`.
pub(crate) fn data_enum<T>(samples: &[T], rows: &[&str], variant_name: fn(&T) -> &'static str)
where
    T: CatchAll + Serialize + DeserializeOwned + PartialEq + Debug,
{
    let mut sampled: Vec<&str> = samples.iter().map(variant_name).collect();
    sampled.sort_unstable();
    let mut expected = rows.to_vec();
    expected.sort_unstable();
    assert_eq!(sampled, expected, "the samples name every row exactly once");
    for sample in samples {
        round_trips(variant_name(sample), sample);
        the_other_form_is_refused(sample);
    }
    let unknown = T::unknown(LATER);
    decodes_to(&format!(r#""{LATER}""#), &unknown);
    round_trips(variant_name(&unknown), &unknown);
    let keyed = format!(r#"{{"{LATER}":{{"b":[1,{{"c":null}}],"a":"x"}}}}"#);
    let null = format!(r#"{{"{LATER}":null}}"#);
    for (format, got) in decoded::<T>(&keyed).into_iter().chain(decoded::<T>(&null)) {
        let name = got.as_ref().map(variant_name);
        assert_eq!(
            name,
            Ok(variant_name(&unknown)),
            "{format} of {keyed}: {got:?}"
        );
    }
    for bad in MALFORMED {
        refused::<T>(bad);
    }
}
