//! `SkipSeed` skips every kind of value and stops where `RawPayload` does.

use serde::Deserialize;
use serde::de::{DeserializeSeed, Visitor};

use super::{MAX_RAW_DEPTH, RawPayload, SkipSeed, nested_json, nested_msgpack_maps};

/// Skips `json` as JSON and `packed` as `MessagePack` with `SkipSeed`, giving both results.
fn skip_both(json: &str, packed: &[u8]) -> [Result<(), String>; 2] {
    let from_json = SkipSeed { depth: 0 }
        .deserialize(&mut serde_json::Deserializer::from_str(json))
        .map_err(|error| error.to_string());
    let from_packed = SkipSeed { depth: 0 }
        .deserialize(&mut rmp_serde::Deserializer::from_read_ref(packed))
        .map_err(|error| error.to_string());
    [from_json, from_packed]
}

/// `depth` nested lists around `0`, as `MessagePack`.
fn nested_msgpack_lists(depth: usize) -> Vec<u8> {
    let mut bytes = vec![0x91; depth];
    bytes.push(0);
    bytes
}

#[test]
fn skip_seed_skips_every_kind_of_value() {
    let json = r#"{"s":"é \"q\"","n":-1,"u":18446744073709551615,"f":0.5,"t":true,"z":null,"l":[1,{"k":[]}],"e":{}}"#;
    let raw: RawPayload = serde_json::from_str(json).unwrap();
    let packed = rmp_serde::to_vec_named(&raw).unwrap();
    assert_eq!(skip_both(json, &packed), [Ok(()), Ok(())]);
    let bin =
        RawPayload::Map(vec![(RawPayload::Bytes(vec![7; 16].into()), RawPayload::U64(1))].into());
    let packed = rmp_serde::to_vec_named(&bin).unwrap();
    let skipped =
        SkipSeed { depth: 0 }.deserialize(&mut rmp_serde::Deserializer::from_read_ref(&packed));
    assert!(skipped.is_ok(), "{skipped:?}");
}

#[test]
fn skip_seed_stops_where_raw_payload_does() {
    let deepest = (
        nested_json(MAX_RAW_DEPTH),
        nested_msgpack_lists(MAX_RAW_DEPTH),
    );
    assert_eq!(skip_both(&deepest.0, &deepest.1), [Ok(()), Ok(())]);
    let too_deep = (
        nested_json(MAX_RAW_DEPTH + 1),
        nested_msgpack_lists(MAX_RAW_DEPTH + 1),
    );
    let raw_errors = [
        serde_json::from_str::<RawPayload>(&too_deep.0)
            .unwrap_err()
            .to_string(),
        rmp_serde::from_slice::<RawPayload>(&too_deep.1)
            .unwrap_err()
            .to_string(),
    ];
    let skip_errors = skip_both(&too_deep.0, &too_deep.1).map(Result::unwrap_err);
    assert_eq!(skip_errors, raw_errors, "the same error as RawPayload");
    assert!(
        skip_errors[0].contains("deeper than 100 levels"),
        "{}",
        skip_errors[0]
    );
    let maps = nested_msgpack_maps(MAX_RAW_DEPTH + 1);
    let skipped =
        SkipSeed { depth: 0 }.deserialize(&mut rmp_serde::Deserializer::from_read_ref(&maps));
    assert!(
        skipped
            .unwrap_err()
            .to_string()
            .contains("deeper than 100 levels")
    );
}

#[test]
fn skip_seed_consumes_a_messagepack_ext_value_whole() {
    // A fixext1 (0xd4) of type 5 holding 0x01, then the number 7: the skip reads all of the ext,
    // so the 7 is next.
    let packed = [0xd4, 0x05, 0x01, 0x07];
    let mut de = rmp_serde::Deserializer::from_read_ref(&packed);
    SkipSeed { depth: 0 }.deserialize(&mut de).unwrap();
    assert_eq!(u8::deserialize(&mut de).unwrap(), 7);
}

#[test]
fn skip_seed_visit_some_skips_the_value_inside() {
    // No format hands `deserialize_any` a `Some`, so the test calls `visit_some` directly: it
    // skips the value inside with the same depth limit, and leaves nothing of it unread.
    let too_deep = nested_json(MAX_RAW_DEPTH + 1);
    let mut de = serde_json::Deserializer::from_str(&too_deep);
    let skipped = SkipSeed { depth: 0 }.visit_some(&mut de);
    assert!(
        skipped
            .unwrap_err()
            .to_string()
            .contains("deeper than 100 levels")
    );
    let mut de = serde_json::Deserializer::from_str("[1,2] ");
    SkipSeed { depth: 0 }.visit_some(&mut de).unwrap();
    de.end().unwrap();
}
