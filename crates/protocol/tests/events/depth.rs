//! Nesting limits: a catch-all's unknown payload stops at `MAX_RAW_DEPTH` in JSON and
//! `MessagePack` alike, so a newer Clauseo's `RawPayload`, encoded at most
//! `MAX_RAW_ENCODE_DEPTH` deep, decodes here under up to 8 levels of its own types; a reader
//! with `set_max_depth(MAX_MSGPACK_DEPTH)` refuses a deep unknown field with an error instead of
//! overflowing a debug build's 2 MiB thread, and `serde_json` skips such a field without
//! recursing.

use clauseo_protocol::{
    ContentBlock, DetachReason, DomainEvent, ElicitationRequest, Envelope, MAX_MSGPACK_DEPTH,
    MAX_RAW_DEPTH, MAX_RAW_ENCODE_DEPTH, RawPayload,
};
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::examples::every_event;
use crate::fixtures::{entry, envelope, err_contains, packed_raw, unknown_event};

/// The string whose `MessagePack` form `with_nested` replaces.
const MARKER: &str = "@@nested@@";

/// `n` lists of one item each around a `nil`, in `MessagePack`.
fn nested_bytes(n: usize) -> Vec<u8> {
    let mut bytes = vec![0x91; n];
    bytes.push(0xc0);
    bytes
}

/// `value` in `MessagePack`, with the string `MARKER` replaced by `n` nested lists.
fn with_nested<T: Serialize>(value: &T, n: usize) -> Vec<u8> {
    let bytes = rmp_serde::to_vec_named(value).unwrap();
    let marker = rmp_serde::to_vec(MARKER).unwrap();
    let at = bytes
        .windows(marker.len())
        .position(|window| window == marker.as_slice())
        .unwrap();
    [&bytes[..at], &nested_bytes(n), &bytes[at + marker.len()..]].concat()
}

/// Reads `bytes` the way the contract on `MAX_MSGPACK_DEPTH` says every reader does.
fn read_limited<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, String> {
    let mut deserializer = rmp_serde::Deserializer::from_read_ref(bytes);
    deserializer.set_max_depth(MAX_MSGPACK_DEPTH);
    T::deserialize(&mut deserializer).map_err(|error| error.to_string())
}

/// Runs `f` on a thread with a debug build's default test stack, 2 MiB.
fn on_2_mib<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(f)
        .unwrap()
        .join()
        .unwrap()
}

/// The example envelope of `kind`, as an editable `RawPayload`.
fn example(kind: &str) -> RawPayload {
    let event = every_event()
        .into_iter()
        .find(|event| event.kind() == kind)
        .unwrap();
    packed_raw(&envelope(event))
}

/// `n` nested JSON lists around a `null`.
fn nested_json(n: usize) -> String {
    format!("{}null{}", "[".repeat(n), "]".repeat(n))
}

#[test]
fn an_unknown_kind_nests_at_most_max_raw_depth_in_both_formats() {
    for (depth, ok) in [(MAX_RAW_DEPTH, true), (MAX_RAW_DEPTH + 1, false)] {
        let json = format!(r#"{{"zz":{}}}"#, nested_json(depth));
        let from_json = serde_json::from_str::<DetachReason>(&json);
        let packed = with_nested(
            &RawPayload::Map(
                vec![(RawPayload::Str("zz".into()), RawPayload::Str(MARKER.into()))].into(),
            ),
            depth,
        );
        let from_packed = read_limited::<DetachReason>(&packed);
        assert_eq!(
            from_json.is_ok(),
            ok,
            "JSON at depth {depth}: {from_json:?}"
        );
        assert_eq!(
            from_packed.is_ok(),
            ok,
            "MessagePack at depth {depth}: {from_packed:?}"
        );
    }
}

#[test]
fn deep_unknown_kind_in_an_envelope_is_an_error() {
    let mut packed = example("agent_session_detached");
    let payload = entry(entry(&mut packed, "event"), "agent_session_detached");
    *entry(payload, "reason") = RawPayload::Map(
        vec![(RawPayload::Str("zz".into()), RawPayload::Str(MARKER.into()))].into(),
    );
    let bytes = with_nested(&packed, 1000);
    let limited = bytes.clone();
    let read = on_2_mib(move || read_limited::<Envelope>(&limited));
    err_contains(read, &format!("nest deeper than {MAX_RAW_DEPTH} levels"));
    // The catch-all counts on its own, so even rmp-serde's default reader refuses it.
    let read =
        on_2_mib(move || rmp_serde::from_slice::<Envelope>(&bytes).map_err(|e| e.to_string()));
    err_contains(read, &format!("nest deeper than {MAX_RAW_DEPTH} levels"));
}

#[test]
fn deep_unknown_field_in_an_envelope_is_an_error() {
    let mut packed = example("turn_started");
    let payload = entry(entry(&mut packed, "event"), "turn_started");
    let RawPayload::Map(fields) = payload else {
        panic!("not a map: {payload:?}");
    };
    let mut with_extra = fields.to_vec();
    with_extra.push((
        RawPayload::Str("extra".into()),
        RawPayload::Str(MARKER.into()),
    ));
    *fields = with_extra.into();
    let bytes = with_nested(&packed, 1000);
    let read = on_2_mib(move || read_limited::<Envelope>(&bytes));
    let error = read.unwrap_err();
    assert!(error.contains("depth"), "{error}");
}

/// `serde_json` skips a field an event does not know without recursing, so even a million nested
/// lists there decode on a 2 MiB thread; `MAX_RAW_DEPTH` bounds only what is kept.
#[test]
fn json_skips_a_deep_unknown_field_without_recursing() {
    let json = format!(
        r#"{{"exited":{{"stderr_tail":"","x":{}}}}}"#,
        nested_json(1_000_000)
    );
    let read = on_2_mib(move || serde_json::from_str::<DetachReason>(&json).is_ok());
    assert!(read);
}

#[test]
fn a_payload_at_max_raw_depth_in_an_envelope_decodes() {
    let unknown = unknown_event("later_kind", RawPayload::Str(MARKER.into()));
    let bytes = with_nested(&envelope(unknown), MAX_RAW_DEPTH);
    let read = on_2_mib(move || read_limited::<Envelope>(&bytes));
    let read = read.unwrap();
    let DomainEvent::Unknown { payload, .. } = &read.event else {
        panic!("not unknown: {:?}", read.event);
    };
    let mut depth = 0;
    let mut value = payload;
    while let RawPayload::Seq(items) = value {
        depth += 1;
        value = &items[0];
    }
    assert_eq!((depth, value), (MAX_RAW_DEPTH, &RawPayload::Null));
}

/// `{kind: {"message": "m", field: <n nested lists>}}` in JSON and in `MessagePack`.
fn wrapped(kind: &str, field: &str, n: usize) -> (String, Vec<u8>) {
    let json = format!(
        r#"{{"{kind}":{{"message":"m","{field}":{}}}}}"#,
        nested_json(n)
    );
    let inner = RawPayload::Map(
        vec![
            (
                RawPayload::Str("message".into()),
                RawPayload::Str("m".into()),
            ),
            (
                RawPayload::Str(field.into()),
                RawPayload::Str(MARKER.into()),
            ),
        ]
        .into(),
    );
    let outer = RawPayload::Map(vec![(RawPayload::Str(kind.into()), inner)].into());
    (json, with_nested(&outer, n))
}

/// A newer Clauseo's `ElicitationRequest::FormV2 { schema }` encodes its schema at most
/// `MAX_RAW_ENCODE_DEPTH` deep; under its kind, unknown here, it takes two more levels and still
/// decodes to the catch-all in both formats. A schema of 100 lists, which no Clauseo encodes,
/// fails to decode.
#[test]
fn a_form_v2_schema_at_the_encode_limit_decodes_to_the_catch_all_and_one_of_100_fails() {
    for (depth, ok) in [(MAX_RAW_ENCODE_DEPTH, true), (100, false)] {
        let (json, packed) = wrapped("form_v2", "schema", depth);
        let from_json = serde_json::from_str::<ElicitationRequest>(&json);
        let from_packed = read_limited::<ElicitationRequest>(&packed);
        assert_eq!(from_json.is_ok(), ok, "JSON at {depth}: {from_json:?}");
        assert_eq!(
            from_packed.is_ok(),
            ok,
            "MessagePack at {depth}: {from_packed:?}"
        );
        if ok {
            let unknown = format!("{:?}", from_json.unwrap());
            assert!(
                unknown.contains("Unknown") && unknown.contains("form_v2"),
                "{unknown}"
            );
            assert_eq!(format!("{:?}", from_packed.unwrap()), unknown);
        }
    }
}

/// A struct variant a newer Clauseo adds to `ContentBlock` with a `RawPayload` field at
/// `MAX_RAW_ENCODE_DEPTH` decodes to `Unsupported` and keeps `raw` in both formats, though the
/// whole block nests 2 levels deeper than the encode limit; a field of 99 lists fails to decode.
#[test]
fn a_content_block_struct_field_at_the_encode_limit_keeps_raw_and_one_of_99_fails() {
    for (depth, ok) in [(MAX_RAW_ENCODE_DEPTH, true), (99, false)] {
        let (json, packed) = wrapped("hologram", "field", depth);
        let from_json = serde_json::from_str::<ContentBlock>(&json);
        let from_packed = read_limited::<ContentBlock>(&packed);
        assert_eq!(from_json.is_ok(), ok, "JSON at {depth}: {from_json:?}");
        assert_eq!(
            from_packed.is_ok(),
            ok,
            "MessagePack at {depth}: {from_packed:?}"
        );
        if ok {
            let block = from_json.unwrap();
            let ContentBlock::Unsupported { raw: Some(raw), .. } = &block else {
                panic!("no raw kept: {block:?}");
            };
            assert_eq!(raw, &clauseo_protocol::TextOrBlob::Inline(json.clone()));
            assert_eq!(from_packed.unwrap(), block);
        }
    }
}
