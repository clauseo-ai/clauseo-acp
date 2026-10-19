//! `RawPayload` round trips: JSON and `MessagePack` write back the bytes they were read from.

use serde::Serialize;

use super::RawPayload;

/// Reads `bytes` as `MessagePack` into a `RawPayload` and writes it back.
fn msgpack_round_trip(bytes: &[u8]) -> Vec<u8> {
    let raw: RawPayload = rmp_serde::from_slice(bytes).unwrap();
    rmp_serde::to_vec_named(&raw).unwrap()
}

#[test]
fn json_round_trips_to_identical_bytes() {
    let json = concat!(
        r#"{"zeta":1,"alpha":[1,[2,[3,-4]],{"k":null}],"min":-9223372036854775808,"#,
        r#""max":18446744073709551615,"f":1.5,"g":-0.25,"h":1e+300,"t":true,"#,
        r#""s":"é \"q\" \\ \n","empty":{},"list":[]}"#
    );
    let raw: RawPayload = serde_json::from_str(json).unwrap();
    let RawPayload::Map(entries) = &raw else {
        panic!("not a map: {raw:?}")
    };
    let keys: Vec<_> = entries.iter().map(|(key, _)| key.clone()).collect();
    assert_eq!(
        keys.first(),
        Some(&RawPayload::Str("zeta".into())),
        "order kept"
    );
    assert!(entries.contains(&(RawPayload::Str("min".into()), RawPayload::I64(i64::MIN))));
    assert!(entries.contains(&(RawPayload::Str("max".into()), RawPayload::U64(u64::MAX))));
    assert_eq!(serde_json::to_string(&raw).unwrap(), json);
    assert_eq!(raw.to_json_text().unwrap(), json);
}

#[test]
fn msgpack_round_trips_to_identical_bytes() {
    let value = RawPayload::Map(
        vec![
            (RawPayload::Str("zeta".into()), RawPayload::U64(1)),
            (
                RawPayload::Str("alpha".into()),
                RawPayload::Seq(
                    vec![
                        RawPayload::I64(-4),
                        RawPayload::Seq(vec![RawPayload::Null, RawPayload::Bool(false)].into()),
                    ]
                    .into(),
                ),
            ),
            (RawPayload::Str("min".into()), RawPayload::I64(i64::MIN)),
            (RawPayload::Str("max".into()), RawPayload::U64(u64::MAX)),
            (RawPayload::Str("f".into()), RawPayload::F64(-0.1)),
            (
                RawPayload::Str("bin".into()),
                RawPayload::Bytes(vec![0, 255, 7].into()),
            ),
            (RawPayload::U64(7), RawPayload::Str("integer key".into())),
        ]
        .into(),
    );
    let bytes = rmp_serde::to_vec_named(&value).unwrap();
    assert_eq!(rmp_serde::from_slice::<RawPayload>(&bytes).unwrap(), value);
    assert_eq!(msgpack_round_trip(&bytes), bytes);
}

#[test]
fn msgpack_of_a_derived_struct_round_trips() {
    // What R7 stores: a named-field struct with a binary uuid and small integers.
    #[derive(Serialize)]
    struct Stored {
        id: uuid::Uuid,
        n: u8,
        neg: i32,
        name: &'static str,
        tags: Vec<&'static str>,
    }
    let stored = Stored {
        id: uuid::Uuid::from_u128(0x0190_0000_0000_7000_8000_0000_0000_0001),
        n: 200,
        neg: -70_000,
        name: "x",
        tags: vec!["b", "a"],
    };
    let bytes = rmp_serde::to_vec_named(&stored).unwrap();
    let raw: RawPayload = rmp_serde::from_slice(&bytes).unwrap();
    assert!(matches!(&raw, RawPayload::Map(entries) if entries.len() == 5));
    assert_eq!(msgpack_round_trip(&bytes), bytes);
}

#[test]
fn msgpack_float32_widens_to_float64() {
    // rmp-serde reports a float32 as f64, so it writes back as float64 (0xcb): the one value
    // rmp-serde itself writes that does not round-trip byte for byte.
    let bytes = rmp_serde::to_vec_named(&1.5f32).unwrap();
    assert_eq!(bytes, [0xca, 0x3f, 0xc0, 0x00, 0x00]);
    assert_eq!(
        rmp_serde::from_slice::<RawPayload>(&bytes).unwrap(),
        RawPayload::F64(1.5)
    );
    assert_eq!(
        msgpack_round_trip(&bytes),
        [0xcb, 0x3f, 0xf8, 0, 0, 0, 0, 0, 0]
    );
}

#[test]
fn non_canonical_input_writes_back_canonical() {
    // A wider integer encoding than needed comes back in the shortest form.
    assert_eq!(msgpack_round_trip(&[0xcd, 0x00, 0x05]), [0x05]);
    assert_eq!(msgpack_round_trip(&[0xd0, 0x05]), [0x05]);
    // JSON: number spellings and escapes come back as serde_json writes them.
    let raw: RawPayload = serde_json::from_str(r#"[1e2, 1.50, "\u00e9", 2.0]"#).unwrap();
    assert_eq!(raw.to_json_text().unwrap(), r#"[100.0,1.5,"é",2.0]"#);
}

#[test]
fn json_text_of_a_binary_key_is_an_error() {
    let value = RawPayload::Map(vec![(RawPayload::Bytes(vec![1].into()), RawPayload::Null)].into());
    assert!(matches!(
        value.to_json_text(),
        Err(crate::Error::NotJson(_))
    ));
    let bytes = RawPayload::Seq(vec![RawPayload::Bytes(vec![1, 2].into())].into());
    assert_eq!(bytes.to_json_text().unwrap(), "[[1,2]]");
}

#[test]
fn serde_json_value_cannot_do_this() {
    // The reason RawPayload exists: Value sorts keys and refuses binary data.
    let value: serde_json::Value = serde_json::from_str(r#"{"b":1,"a":2}"#).unwrap();
    assert_eq!(value.to_string(), r#"{"a":2,"b":1}"#);
    let bin = rmp_serde::to_vec_named(&RawPayload::Bytes(vec![1].into())).unwrap();
    assert!(rmp_serde::from_slice::<serde_json::Value>(&bin).is_err());
}

#[test]
fn raw_payload_is_24_bytes() {
    // Boxed slices and strings are 16 bytes each; a Vec or String would make it 32.
    assert_eq!(std::mem::size_of::<RawPayload>(), 24);
}
