//! The values that change form on the way through a `RawPayload`, each pinned as `RawPayload`'s
//! doc lists it, and the 128-bit integers both visitors refuse.

use serde::Deserialize;
use serde::de::value::Error as ValueError;
use serde::de::{DeserializeSeed, IntoDeserializer};

use super::{RawPayload, SkipSeed, msgpack_round_trip};

#[test]
fn a_msgpack_ext_value_decodes_as_a_type_and_bytes_pair_and_writes_back_as_an_array() {
    // fixext1 of type 5 holding 0x01.
    let ext = [0xd4, 0x05, 0x01];
    assert_eq!(
        rmp_serde::from_slice::<RawPayload>(&ext).unwrap(),
        RawPayload::Seq(vec![RawPayload::U64(5), RawPayload::Bytes(vec![1].into())].into())
    );
    assert_eq!(msgpack_round_trip(&ext), [0x92, 0x05, 0xc4, 0x01, 0x01]);
}

#[test]
fn a_msgpack_str_that_is_not_utf8_decodes_as_bytes_and_writes_back_as_bin() {
    let text = [0xa2, 0xff, 0xfe];
    assert_eq!(
        rmp_serde::from_slice::<RawPayload>(&text).unwrap(),
        RawPayload::Bytes(vec![0xff, 0xfe].into())
    );
    assert_eq!(msgpack_round_trip(&text), [0xc4, 0x02, 0xff, 0xfe]);
}

#[test]
fn a_json_integer_outside_i64_and_u64_decodes_as_a_float() {
    let raw: RawPayload = serde_json::from_str("-18446744073709551617").unwrap();
    assert_eq!(raw, RawPayload::F64(-18_446_744_073_709_551_617.0));
    assert_eq!(raw.to_json_text().unwrap(), "-1.8446744073709552e+19");
}

/// `RawPayload` is exempt from `MAX_SAFE`: a `U64` above 2^53 is written as its exact digits,
/// which a reader whose numbers are 64-bit floats (the webview) reads as a different number.
#[test]
fn a_u64_above_max_safe_is_written_exactly_and_reaches_a_float_reader_inexact() {
    let maximum = (1_u64 << 53) + 1;
    assert!(maximum > crate::MAX_SAFE);
    let schema =
        RawPayload::Map(vec![(RawPayload::Str("maximum".into()), RawPayload::U64(maximum))].into());
    let json = schema.to_json_text().unwrap();
    assert_eq!(json, r#"{"maximum":9007199254740993}"#);
    let as_float: f64 = serde_json::from_str("9007199254740993").unwrap();
    assert_eq!(
        as_float, 9_007_199_254_740_992.0,
        "a float reader rounds it"
    );
}

/// No format sends a 128-bit integer today; `SkipSeed` and `RawPayload` refuse one alike.
#[test]
fn skip_seed_and_raw_payload_both_refuse_128_bit_integers() {
    let signed = || IntoDeserializer::<ValueError>::into_deserializer(1_i128);
    let unsigned = || IntoDeserializer::<ValueError>::into_deserializer(1_u128);
    assert!(SkipSeed { depth: 0 }.deserialize(signed()).is_err());
    assert!(RawPayload::deserialize(signed()).is_err());
    assert!(SkipSeed { depth: 0 }.deserialize(unsigned()).is_err());
    assert!(RawPayload::deserialize(unsigned()).is_err());
}
