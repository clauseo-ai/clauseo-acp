//! JSON copies of values read from `MessagePack`: NaN, the infinities and a scalar or binary map
//! key come out in JSON's nearest form instead of failing the event, a list or map key fails the
//! decode, an unknown block's payload decodes one level under `MAX_RAW_DEPTH`, and its copy is
//! kept only within `INLINE_LIMIT_BYTES`.

use clauseo_protocol::{
    ContentBlock, DomainEvent, Envelope, HostEvent, INLINE_LIMIT_BYTES, MAX_RAW_DEPTH, RawFrom,
    RawPayload, TextOrBlob, escaped_len,
};

use crate::fixtures::{envelope, unknown_event};

/// The JSON copy of a 16-byte binary key of sevens: the text of its list of byte numbers.
pub(crate) const ID_KEY: &str = "[7,7,7,7,7,7,7,7,7,7,7,7,7,7,7,7]";

/// `{key: inner}` as a `RawPayload` map.
fn one_key(key: &str, inner: RawPayload) -> RawPayload {
    RawPayload::Map(vec![(RawPayload::Str(key.into()), inner)].into())
}

/// `{<16 bytes of 7>: 1}`: a map keyed by an id, which only `MessagePack` holds.
fn by_id() -> RawPayload {
    RawPayload::Map(vec![(RawPayload::Bytes(vec![7; 16].into()), RawPayload::U64(1))].into())
}

/// `value` written as `MessagePack` and read back as `T`.
fn via_msgpack<T: serde::de::DeserializeOwned>(value: &RawPayload) -> Result<T, String> {
    rmp_serde::from_slice(&rmp_serde::to_vec_named(value).unwrap()).map_err(|e| e.to_string())
}

/// An unknown block of kind `hologram` whose `raw` text is `json`.
fn hologram(json: &str) -> ContentBlock {
    ContentBlock::Unsupported {
        kind: "hologram".to_owned(),
        raw_from: RawFrom::Clauseo,
        raw: Some(TextOrBlob::Inline(json.to_owned())),
    }
}

/// `K_n` as `MessagePack`: `K_0 = 1`, `K_n = {K_(n-1): nil}`, keys inside keys.
fn key_chain(n: usize) -> Vec<u8> {
    let mut bytes = vec![0x81; n];
    bytes.push(0x01);
    bytes.extend(vec![0xc0; n]);
    bytes
}

/// `{kind: K_40}` as `MessagePack`: about 90 bytes.
fn under_key_chain(kind: &str) -> Vec<u8> {
    let mut bytes = rmp_serde::to_vec_named(&one_key(kind, RawPayload::Null)).unwrap();
    bytes.pop();
    bytes.extend(key_chain(40));
    bytes
}

/// The decode error for a map key that is a list or a map.
const LIST_OR_MAP_KEY: &str = "RawPayload: a list or a map as a map key";

/// Asserts that `result` is the decode error for a list or map key, not the encoder's.
fn refused_key<T: std::fmt::Debug>(result: Result<T, String>) {
    let error = result.unwrap_err();
    assert!(error.contains(LIST_OR_MAP_KEY), "{error}");
    assert!(!error.contains("no JSON form"), "{error}");
}

/// A 40-level chain of map keys inside map keys fails to decode, as an unknown block and as an
/// unknown event, at the first key that is a map. A copy that wrote each key as escaped JSON
/// text would double per level, about 2 TB here, and abort.
#[test]
fn a_chain_of_map_keys_fails_to_decode() {
    refused_key(
        rmp_serde::from_slice::<ContentBlock>(&under_key_chain("hologram"))
            .map_err(|e| e.to_string()),
    );
    refused_key(
        rmp_serde::from_slice::<DomainEvent>(&under_key_chain("zz")).map_err(|e| e.to_string()),
    );
}

/// A list or a map as a map key has no JSON form, and no Clauseo type writes one, so a row holding
/// one fails at the read, as an unknown event, in an envelope and as an unknown block, and never
/// reaches a batch whose JSON encode it would fail.
#[test]
fn a_list_or_map_key_fails_to_decode() {
    let list = RawPayload::Seq(vec![RawPayload::U64(1)].into());
    for key in [list, one_key("a", RawPayload::U64(1))] {
        let keyed = RawPayload::Map(vec![(key, RawPayload::U64(1))].into());
        refused_key(via_msgpack::<DomainEvent>(&one_key("zz", keyed.clone())));
        refused_key(via_msgpack::<ContentBlock>(&one_key(
            "hologram",
            keyed.clone(),
        )));
        let unknown = unknown_event("zz", keyed);
        let packed = rmp_serde::to_vec_named(&envelope(unknown)).unwrap();
        refused_key(rmp_serde::from_slice::<Envelope>(&packed).map_err(|e| e.to_string()));
    }
}

/// An integer key and a 16-byte binary key still decode, and their JSON copy writes them as text.
#[test]
fn an_integer_or_binary_key_decodes_and_becomes_text() {
    let keyed = RawPayload::Map(
        vec![
            (RawPayload::U64(7), RawPayload::U64(1)),
            (RawPayload::Bytes(vec![7; 16].into()), RawPayload::U64(2)),
        ]
        .into(),
    );
    let event: DomainEvent = via_msgpack(&one_key("zz", keyed)).unwrap();
    let expected = format!(r#"{{"zz":{{"7":1,"{ID_KEY}":2}}}}"#);
    assert_eq!(serde_json::to_string(&event).unwrap(), expected);
}

/// A binary key's copy equals the string of its byte list: the copy holds the key twice, and
/// `serde_json::Value`, which keeps the last duplicate, drops the first entry.
#[test]
fn a_binary_key_collides_with_the_string_of_its_bytes() {
    let both = RawPayload::Map(
        vec![
            (RawPayload::Bytes(vec![1, 2].into()), RawPayload::U64(1)),
            (RawPayload::Str("[1,2]".into()), RawPayload::U64(2)),
        ]
        .into(),
    );
    let event: DomainEvent = via_msgpack(&one_key("zz", both)).unwrap();
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(json, r#"{"zz":{"[1,2]":1,"[1,2]":2}}"#);
    let value: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(value.to_string(), r#"{"zz":{"[1,2]":2}}"#);
}

#[test]
fn a_float_key_collides_with_its_decimal_string() {
    let both = RawPayload::Map(
        vec![
            (RawPayload::F64(1.5), RawPayload::U64(1)),
            (RawPayload::Str("1.5".into()), RawPayload::U64(2)),
        ]
        .into(),
    );
    let event: DomainEvent = via_msgpack(&one_key("zz", both)).unwrap();
    let json = serde_json::to_string(&event).unwrap();
    assert_eq!(json, r#"{"zz":{"1.5":1,"1.5":2}}"#);
}

#[test]
fn nan_and_the_infinities_become_null_in_a_json_copy() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let event: DomainEvent = via_msgpack(&one_key("zz", RawPayload::F64(value))).unwrap();
        let DomainEvent::Unknown { kind, payload } = &event else {
            panic!("not unknown: {event:?}");
        };
        let RawPayload::F64(read) = payload else {
            panic!("not a float: {payload:?}");
        };
        assert_eq!((kind.as_str(), read.to_bits()), ("zz", value.to_bits()));
        let json = serde_json::to_string(&event).unwrap();
        assert_eq!(json, r#"{"zz":null}"#);
        let back: DomainEvent = serde_json::from_str(&json).unwrap();
        let expected = unknown_event("zz", RawPayload::Null);
        assert_eq!(back, expected);

        let block = one_key("hologram", one_key("x", RawPayload::F64(value)));
        assert_eq!(
            via_msgpack::<ContentBlock>(&block).unwrap(),
            hologram(r#"{"hologram":{"x":null}}"#)
        );
    }
}

/// An unknown event keyed by an id encodes to JSON alone and in an `Events` batch, its key
/// written as the text of its JSON copy; the same block keeps that text as `raw`.
#[test]
fn a_binary_key_becomes_its_json_text_in_json() {
    let event: DomainEvent = via_msgpack(&one_key("zz", by_id())).unwrap();
    assert!(matches!(&event, DomainEvent::Unknown { kind, .. } if kind == "zz"));
    let expected = format!(r#"{{"zz":{{"{ID_KEY}":1}}}}"#);
    assert_eq!(serde_json::to_string(&event).unwrap(), expected);
    let batch = HostEvent::Events(vec![envelope(event)]);
    let json = serde_json::to_string(&batch).unwrap();
    assert!(json.contains(&format!(r#""event":{expected}"#)), "{json}");

    let block = via_msgpack::<ContentBlock>(&one_key("hologram", by_id())).unwrap();
    assert_eq!(
        block,
        hologram(&format!(r#"{{"hologram":{{"{ID_KEY}":1}}}}"#))
    );
}

/// `{"hologram": <depth nested lists around null>}` as JSON text and as `MessagePack`.
fn deep_hologram(depth: usize) -> (String, Vec<u8>) {
    let json = format!(
        r#"{{"hologram":{}null{}}}"#,
        "[".repeat(depth),
        "]".repeat(depth)
    );
    let mut packed = vec![0x81, 0xa8];
    packed.extend(b"hologram");
    packed.extend(vec![0x91; depth]);
    packed.push(0xc0);
    (json, packed)
}

/// A block's payload is read one level down, as it sits in the whole block `raw` keeps:
/// `MAX_RAW_DEPTH - 1` nested lists keep `raw` in both formats, and `MAX_RAW_DEPTH` fail to
/// decode with the decode limit's error, instead of decoding with `raw: None`.
#[test]
fn an_unknown_block_payload_decodes_one_level_under_max_raw_depth() {
    let (json, packed) = deep_hologram(MAX_RAW_DEPTH - 1);
    assert_eq!(
        serde_json::from_str::<ContentBlock>(&json).unwrap(),
        hologram(&json)
    );
    assert_eq!(
        rmp_serde::from_slice::<ContentBlock>(&packed).unwrap(),
        hologram(&json)
    );

    let (deep_json, deep_packed) = deep_hologram(MAX_RAW_DEPTH);
    let errors = [
        serde_json::from_str::<ContentBlock>(&deep_json)
            .map(|_| ())
            .map_err(|e| e.to_string()),
        rmp_serde::from_slice::<ContentBlock>(&deep_packed)
            .map(|_| ())
            .map_err(|e| e.to_string()),
    ];
    for error in errors {
        let error = error.unwrap_err();
        assert!(error.contains("deeper than 100 levels"), "{error}");
    }
}

/// An unknown block keeps `raw` when the text's `escaped_len` is exactly `INLINE_LIMIT_BYTES`
/// and drops it one byte over, in both formats. Escapes count twice over: each newline in the
/// payload is `\n` in `raw` and `\\n` once `raw` is escaped again, 3 bytes.
#[test]
fn an_unknown_block_keeps_raw_at_the_inline_limit_and_drops_it_one_byte_over() {
    // `{"hologram":"` and `"}` cost 19 escaped bytes, each newline 3.
    let newlines = "\n".repeat((INLINE_LIMIT_BYTES - 19) / 3);
    for (text, kept) in [(newlines.clone(), true), (format!("{newlines}x"), false)] {
        let whole = one_key("hologram", RawPayload::Str(text.as_str().into()));
        let json = serde_json::to_string(&whole).unwrap();
        let cost = escaped_len(&json);
        assert_eq!(cost, INLINE_LIMIT_BYTES + usize::from(!kept), "{cost}");
        let expected = ContentBlock::Unsupported {
            kind: "hologram".to_owned(),
            raw_from: RawFrom::Clauseo,
            raw: kept.then(|| TextOrBlob::Inline(json.clone())),
        };
        assert_eq!(
            serde_json::from_str::<ContentBlock>(&json).unwrap(),
            expected
        );
        assert_eq!(via_msgpack::<ContentBlock>(&whole).unwrap(), expected);
    }
}
