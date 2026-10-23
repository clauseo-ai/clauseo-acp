//! `RawPayload` round trips: JSON and `MessagePack` write back the bytes they were read from.

use serde::Serialize;

use super::{AsDecoded, MAX_RAW_DEPTH, MAX_RAW_ENCODE_DEPTH, RawPayload, RawVisitor, SkipSeed};

mod one_way;
mod skip;

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
    // What `store` stores: a named-field struct with a binary uuid and small integers.
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

/// A one-entry map with `key` mapped to `1`.
fn keyed(key: RawPayload) -> RawPayload {
    RawPayload::Map(vec![(key, RawPayload::U64(1))].into())
}

/// JSON writes a scalar or binary map key as a string holding its compact JSON text, at any
/// depth; `MessagePack` keeps the key as it is.
#[test]
fn json_writes_a_scalar_or_binary_key_as_its_json_text() {
    let cases = [
        (RawPayload::U64(7), r#"{"7":1}"#),
        (RawPayload::I64(-7), r#"{"-7":1}"#),
        (RawPayload::Bool(true), r#"{"true":1}"#),
        (RawPayload::F64(1.5), r#"{"1.5":1}"#),
        (RawPayload::F64(f64::NAN), r#"{"null":1}"#),
        (RawPayload::Null, r#"{"null":1}"#),
        (RawPayload::Bytes(vec![1, 2].into()), r#"{"[1,2]":1}"#),
    ];
    for (key, json) in cases {
        let value = keyed(key);
        assert_eq!(value.to_json_text().unwrap(), json, "{value:?}");
        assert_eq!(serde_json::to_string(&value).unwrap(), json, "{value:?}");
        let packed = rmp_serde::to_vec_named(&value).unwrap();
        let back = rmp_serde::from_slice::<RawPayload>(&packed).unwrap();
        assert_eq!(
            format!("{back:?}"),
            format!("{value:?}"),
            "MessagePack keeps the key"
        );
    }
    // A one-way projection: the key reads back as a string, and `7` and `"7"` collide.
    let both = RawPayload::Map(
        vec![
            (RawPayload::U64(7), RawPayload::U64(1)),
            (RawPayload::Str("7".into()), RawPayload::U64(2)),
        ]
        .into(),
    );
    let json = both.to_json_text().unwrap();
    assert_eq!(json, r#"{"7":1,"7":2}"#);
    let back: RawPayload = serde_json::from_str(&json).unwrap();
    let RawPayload::Map(entries) = back else {
        panic!("not a map: {back:?}");
    };
    assert!(
        entries
            .iter()
            .all(|(key, _)| key == &RawPayload::Str("7".into()))
    );
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

/// `depth` nested lists around `0`, as JSON text.
fn nested_json(depth: usize) -> String {
    format!("{}0{}", "[".repeat(depth), "]".repeat(depth))
}

/// `depth` nested one-entry maps (`{"k": ...}`) around `0`, as `MessagePack`.
fn nested_msgpack_maps(depth: usize) -> Vec<u8> {
    let mut bytes = [0x81, 0xa1, b'k'].repeat(depth);
    bytes.push(0);
    bytes
}

/// Decoding accepts [`MAX_RAW_DEPTH`] lists or maps in both formats and refuses one more; what
/// decodes there writes back under the decode limit, as a catch-all's payload does.
#[test]
fn decoding_accepts_max_raw_depth_and_refuses_one_more_in_both_formats() {
    let deepest = nested_json(MAX_RAW_DEPTH);
    let raw: RawPayload = serde_json::from_str(&deepest).unwrap();
    assert_eq!(serde_json::to_string(&AsDecoded(&raw)).unwrap(), deepest);
    let packed = rmp_serde::to_vec_named(&AsDecoded(&raw)).unwrap();
    assert_eq!(rmp_serde::from_slice::<RawPayload>(&packed).unwrap(), raw);
    let maps = nested_msgpack_maps(MAX_RAW_DEPTH);
    let decoded: RawPayload = rmp_serde::from_slice(&maps).unwrap();
    assert_eq!(rmp_serde::to_vec_named(&AsDecoded(&decoded)).unwrap(), maps);

    let too_deep = format!("[{deepest}]");
    let packed = [&[0x91][..], &packed].concat();
    let errors = [
        serde_json::from_str::<RawPayload>(&too_deep)
            .unwrap_err()
            .to_string(),
        rmp_serde::from_slice::<RawPayload>(&packed)
            .unwrap_err()
            .to_string(),
        rmp_serde::from_slice::<RawPayload>(&nested_msgpack_maps(MAX_RAW_DEPTH + 1))
            .unwrap_err()
            .to_string(),
    ];
    for error in errors {
        assert!(error.contains("deeper than 100 levels"), "{error}");
    }
}

/// An unknown event (`{"zz": ...}`) whose payload is 1000 nested `header` (array32 or map32,
/// each claiming 2^32 - 1 entries) and nothing else: 5,004 bytes.
fn header_bomb(header: u8) -> Vec<u8> {
    let mut bytes = vec![0x81, 0xa2, b'z', b'z'];
    bytes.extend([header, 0xff, 0xff, 0xff, 0xff].repeat(1000));
    bytes
}

/// 1000 nested array32 or map32 headers stop at level 101 with the depth error instead of
/// overflowing a debug build's stack. The claimed lengths are tested by
/// `reserve_never_trusts_a_claimed_length` and `visitors_never_trust_a_size_hint`.
#[test]
fn nested_length_headers_stop_at_the_depth_limit() {
    for header in [0xdd, 0xdf] {
        let error = rmp_serde::from_slice::<crate::DomainEvent>(&header_bomb(header))
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("deeper than 100 levels"),
            "{header:#x}: {error}"
        );
    }
    // A claim alone, without the nesting, fails at the end of the input.
    let short = [0xdd, 0xff, 0xff, 0xff, 0xff, 0x01];
    assert!(rmp_serde::from_slice::<RawPayload>(&short).is_err());
}

/// A list or map reserves at most 16 slots whatever length its header claims, so a 5-byte header
/// claiming 2^32 - 1 items allocates nothing large before its items arrive.
#[test]
fn reserve_never_trusts_a_claimed_length() {
    let cases = [
        (None, 0),
        (Some(0), 0),
        (Some(3), 3),
        (Some(16), 16),
        (Some(17), 16),
        (Some(u32::MAX as usize), 16),
    ];
    for (hint, slots) in cases {
        assert_eq!(super::reserve(hint), slots, "{hint:?}");
    }
    // rmp-serde passes an array32 header's claim on as the hint; one item still decodes.
    let one = [0xdd, 0x00, 0x00, 0x00, 0x01, 0x07];
    assert_eq!(
        rmp_serde::from_slice::<RawPayload>(&one).unwrap(),
        RawPayload::Seq(vec![RawPayload::U64(7)].into())
    );
}

/// An empty iterator that claims `usize::MAX` items.
struct Claims<T>(std::marker::PhantomData<T>);

impl<T> Iterator for Claims<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        None
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (usize::MAX, Some(usize::MAX))
    }
}

/// The visitors reserve through `reserve`: a list or map whose size hint is `usize::MAX` decodes
/// instead of panicking with a capacity overflow in `Vec::with_capacity`.
#[test]
fn visitors_never_trust_a_size_hint() {
    use serde::Deserialize;
    use serde::de::value::{Error, MapDeserializer, SeqDeserializer};

    let seq = SeqDeserializer::<_, Error>::new(Claims::<u8>(std::marker::PhantomData));
    assert_eq!(
        RawPayload::deserialize(seq).unwrap(),
        RawPayload::Seq(Box::new([]))
    );
    let map = MapDeserializer::<_, Error>::new(Claims::<(u8, u8)>(std::marker::PhantomData));
    assert_eq!(
        RawPayload::deserialize(map).unwrap(),
        RawPayload::Map(Box::new([]))
    );
}

/// A list or a map as a map key has no JSON form: the JSON copy of a value built by hand with one
/// fails, at the top or nested, with the encoder's error. `MessagePack` writes it, and decoding
/// refuses it with its own error.
#[test]
fn json_refuses_a_list_or_map_key() {
    let list = RawPayload::Seq(vec![RawPayload::U64(1)].into());
    for key in [list, keyed(RawPayload::U64(1))] {
        let top = keyed(key);
        let nested = RawPayload::Seq(vec![top.clone()].into());
        for value in [top, nested] {
            let error = value.to_json_text().unwrap_err();
            assert!(matches!(error, crate::Error::NotJson(_)), "{error:?}");
            assert!(error.to_string().contains("map key"), "{error}");
            let error = serde_json::to_string(&value).unwrap_err().to_string();
            assert!(
                error.contains("a list or a map as a map key has no JSON form"),
                "{error}"
            );
            let packed = rmp_serde::to_vec_named(&value).unwrap();
            let error = rmp_serde::from_slice::<RawPayload>(&packed)
                .unwrap_err()
                .to_string();
            assert!(
                error.ends_with("RawPayload: a list or a map as a map key"),
                "{error}"
            );
        }
    }
}

/// Encoding stops 8 levels under decoding: [`MAX_RAW_ENCODE_DEPTH`] lists or maps encode in both
/// formats and one more fails, an empty list at that level included; a catch-all's decoded
/// value (`AsDecoded`) encodes up to [`MAX_RAW_DEPTH`] and fails one level past it.
#[test]
fn encoding_stops_at_max_raw_encode_depth_and_a_decoded_catch_all_at_max_raw_depth() {
    let lists = |depth: usize, inner: RawPayload| {
        (0..depth).fold(inner, |inner, _| RawPayload::Seq(vec![inner].into()))
    };
    let maps = |depth: usize| {
        (0..depth).fold(RawPayload::Null, |inner, _| {
            RawPayload::Map(vec![(RawPayload::Str("k".into()), inner)].into())
        })
    };
    assert_eq!(MAX_RAW_ENCODE_DEPTH, MAX_RAW_DEPTH - 8);
    let empty = || RawPayload::Seq(Box::new([]));
    for (limit, decoded) in [(MAX_RAW_ENCODE_DEPTH, false), (MAX_RAW_DEPTH, true)] {
        let encode = |value: &RawPayload| {
            let (json, packed) = if decoded {
                let value = AsDecoded(value);
                (
                    serde_json::to_string(&value),
                    rmp_serde::to_vec_named(&value),
                )
            } else {
                (serde_json::to_string(value), rmp_serde::to_vec_named(value))
            };
            [
                json.map_err(|e| e.to_string()),
                packed.map(|_| String::new()).map_err(|e| e.to_string()),
            ]
        };
        for value in [lists(limit, RawPayload::Null), maps(limit)] {
            let [json, packed] = encode(&value);
            assert_eq!(
                serde_json::from_str::<RawPayload>(&json.unwrap()).unwrap(),
                value
            );
            assert!(packed.is_ok(), "{limit}: {packed:?}");
        }
        for value in [
            lists(limit + 1, RawPayload::Null),
            lists(limit, empty()),
            maps(limit + 1),
        ] {
            for error in encode(&value) {
                let error = error.unwrap_err();
                assert!(
                    error.contains(&format!("deeper than {limit} levels")),
                    "{error}"
                );
            }
        }
    }
}

#[test]
fn skip_and_raw_visitors_say_they_take_any_value() {
    // Both visitors accept every value, so no format reaches `expecting` through an error;
    // the text is what a caller's error would name.
    let skip = format!("{}", &SkipSeed { depth: 0 } as &dyn serde::de::Expected);
    assert_eq!(skip, "any value");
    let raw = format!("{}", &RawVisitor { depth: 0 } as &dyn serde::de::Expected);
    assert_eq!(raw, "any value");
}
