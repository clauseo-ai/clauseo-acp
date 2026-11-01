//! protocol: wire types decoded with serde, from JSON (serde_json::from_slice, as the webview and
//! host boundary would read them) and from MessagePack (rmp_serde with `MAX_MSGPACK_DEPTH`, the
//! depth limit the store's codec uses), plus the `FromStr` parsers of the small ids.
//!
//! Items driven: the `Deserialize` impls of `EnvelopeRead`, `Envelope`, `CommandEnvelope`,
//! `HostQuery`, `HostEvent`, `CommandOutcome`, `RawPayload`, `AccountLimits`, `ContentBlock`,
//! `Subscribe`, `SubscribeEvents` and `Ack`; `CommandEnvelope::validate`;
//! `RawPayload::to_json_text`; and the `FromStr` impls of `StreamId`, `ThreadId`, `BlobHash`,
//! `Title`, `AgentSessionId` and `AgentChoiceId`.
//!
//! Input: byte 0 is the selector, the rest is the data. The low nibble picks the type: 0
//! EnvelopeRead, 1 Envelope, 2 CommandEnvelope, 3 HostQuery, 4 HostEvent, 5 CommandOutcome,
//! 6 RawPayload, 7 AccountLimits, 8 ContentBlock, 9 Subscribe, SubscribeEvents and Ack, 10
//! StreamId, ThreadId and BlobHash, 11 Title, AgentSessionId and AgentChoiceId, 12 to 15
//! RawPayload again, printed with `to_json_text` and read back. Bit 4 set means MessagePack, not
//! JSON.
//!
//! Invariants, on every successful decode:
//!   - the value encodes (`to_vec`, or `to_vec_named` for MessagePack), except for the depth gap
//!     below,
//!   - decode(encode(v)) succeeds and equals v. JSON floats may differ by a few ULP, as the crate
//!     documents, and a NaN inside a RawPayload compares by its Debug form. A value read from
//!     MessagePack is only checked through MessagePack, since its JSON copy is one-way,
//!   - an `EnvelopeRead::Newer` has a schema version above `SCHEMA_VERSION`,
//!   - `CommandEnvelope::validate` gives the same verdict after a JSON round trip,
//!   - a parsed id prints as a string that parses back to the same value,
//!   - the JSON text from `to_json_text` decodes again.
//!
//! Depth gap: a RawPayload decodes up to `MAX_RAW_DEPTH` levels but encodes only up to
//! `MAX_RAW_ENCODE_DEPTH`, on purpose (crates/protocol/src/raw.rs). An encode error that says
//! "nest deeper than" is therefore skipped, and any other encode error fails.
#![no_main]
#![expect(
    clippy::disallowed_methods,
    reason = "fuzz harness, not app code: fuzz_target! writes its debug file with std::fs"
)]

use std::fmt::Debug;
use std::str::FromStr;

use clauseo_protocol::{
    AccountLimits, Ack, AgentChoiceId, AgentSessionId, BlobHash, CommandEnvelope, CommandOutcome,
    ContentBlock, Envelope, EnvelopeRead, HostEvent, HostQuery, MAX_MSGPACK_DEPTH, RawPayload,
    SCHEMA_VERSION, StreamId, Subscribe, SubscribeEvents, ThreadId, Title,
};
use libfuzzer_sys::fuzz_target;
use serde::Serialize;
use serde::de::DeserializeOwned;

fn msgpack_decode<T: DeserializeOwned>(bytes: &[u8]) -> Option<T> {
    let mut de = rmp_serde::Deserializer::from_read_ref(bytes);
    de.set_max_depth(MAX_MSGPACK_DEPTH);
    T::deserialize(&mut de).ok()
}

fn close_f64(a: f64, b: f64) -> bool {
    if a == b {
        return true;
    }
    let (ia, ib) = (a.to_bits() as i64, b.to_bits() as i64);
    (ia - ib).abs() <= 4
}

/// Structural equality of two JSON values, with floats within a few ULP.
fn json_close(a: &serde_json::Value, b: &serde_json::Value) -> bool {
    use serde_json::Value::*;
    match (a, b) {
        (Number(x), Number(y)) => {
            if x == y {
                return true;
            }
            match (x.as_f64(), y.as_f64()) {
                (Some(p), Some(q)) => close_f64(p, q),
                _ => false,
            }
        }
        (Array(x), Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| json_close(p, q))
        }
        (Object(x), Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| json_close(v, w)))
        }
        _ => a == b,
    }
}

/// The depth gap of a RawPayload (crates/protocol/src/raw.rs documents it): a value decodes up to
/// `MAX_RAW_DEPTH` levels but encodes only up to `MAX_RAW_ENCODE_DEPTH`, on purpose. Such an
/// encode error is skipped. The check is by message, so any other error at that depth still fails.
fn documented_depth_gap(error: &dyn std::fmt::Display) -> bool {
    error.to_string().contains("nest deeper than")
}

fn json_roundtrip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let encoded = match serde_json::to_vec(value) {
        Ok(encoded) => encoded,
        Err(e) if documented_depth_gap(&e) => return,
        Err(e) => panic!("a decoded value does not encode to JSON: {e}: {value:?}"),
    };
    let back: T = serde_json::from_slice(&encoded).unwrap_or_else(|e| {
        panic!(
            "JSON round trip fails to decode: {e}\nvalue: {value:?}\njson: {}",
            String::from_utf8_lossy(&encoded)
        )
    });
    if &back != value {
        let again = serde_json::to_vec(&back).unwrap();
        let a: serde_json::Value = serde_json::from_slice(&encoded).unwrap();
        let b: serde_json::Value = serde_json::from_slice(&again).unwrap();
        assert!(
            json_close(&a, &b),
            "JSON round trip changes the value\nfirst: {value:?}\nback:  {back:?}"
        );
    }
}

fn msgpack_roundtrip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) {
    let encoded = match rmp_serde::to_vec_named(value) {
        Ok(encoded) => encoded,
        Err(e) if documented_depth_gap(&e) => return,
        Err(e) => panic!("a decoded value does not encode to MessagePack: {e}: {value:?}"),
    };
    let back: T = msgpack_decode(&encoded)
        .unwrap_or_else(|| panic!("MessagePack round trip fails to decode: {value:?}"));
    // NaN inside a RawPayload is not equal to itself; compare the Debug form then.
    assert!(
        &back == value || format!("{back:?}") == format!("{value:?}"),
        "MessagePack round trip changes the value\nfirst: {value:?}\nback:  {back:?}"
    );
}

fn check<T: Serialize + DeserializeOwned + PartialEq + Debug>(
    bytes: &[u8],
    msgpack: bool,
) -> Option<T> {
    let value: T = if msgpack {
        msgpack_decode(bytes)?
    } else {
        serde_json::from_slice(bytes).ok()?
    };
    // A JSON copy of MessagePack input is one-way (lib.rs: NaN as null, scalar keys as
    // strings), so values read from MessagePack only take the MessagePack round trip.
    if !msgpack {
        json_roundtrip(&value);
    }
    msgpack_roundtrip(&value);
    Some(value)
}

fn check_str<T: FromStr + ToString + PartialEq + Debug>(bytes: &[u8]) {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return;
    };
    if let Ok(value) = text.parse::<T>() {
        let again = value.to_string();
        let back = again
            .parse::<T>()
            .unwrap_or_else(|_| panic!("{value:?} prints as {again:?}, which does not parse"));
        assert_eq!(back, value);
    }
}

fuzz_target!(|data: &[u8]| {
    let Some((&selector, bytes)) = data.split_first() else {
        return;
    };
    let msgpack = selector & 0x10 != 0;
    match selector & 0x0f {
        0 => {
            let read: Option<EnvelopeRead> = if msgpack {
                msgpack_decode(bytes)
            } else {
                serde_json::from_slice(bytes).ok()
            };
            match read {
                Some(EnvelopeRead::Current(envelope)) => {
                    if !msgpack {
                        json_roundtrip(&envelope);
                    }
                    msgpack_roundtrip(&envelope);
                }
                Some(EnvelopeRead::Newer { schema_version }) => {
                    assert!(
                        schema_version > SCHEMA_VERSION,
                        "Newer with {schema_version}"
                    );
                }
                None => {}
            }
        }
        1 => {
            check::<Envelope>(bytes, msgpack);
        }
        2 => {
            if let Some(command) = check::<CommandEnvelope>(bytes, msgpack) {
                let verdict = command.clone().validate().is_ok();
                // Fails only for the documented depth gap; json_roundtrip already panicked
                // otherwise.
                let Ok(json) = serde_json::to_vec(&command) else {
                    return;
                };
                let back: CommandEnvelope = serde_json::from_slice(&json).unwrap();
                assert_eq!(
                    back.validate().is_ok(),
                    verdict,
                    "validate changes after a round trip"
                );
            }
        }
        3 => {
            check::<HostQuery>(bytes, msgpack);
        }
        4 => {
            check::<HostEvent>(bytes, msgpack);
        }
        5 => {
            check::<CommandOutcome>(bytes, msgpack);
        }
        6 => {
            check::<RawPayload>(bytes, msgpack);
        }
        7 => {
            check::<AccountLimits>(bytes, msgpack);
        }
        8 => {
            check::<ContentBlock>(bytes, msgpack);
        }
        9 => {
            check::<Subscribe>(bytes, msgpack);
            check::<SubscribeEvents>(bytes, msgpack);
            check::<Ack>(bytes, msgpack);
        }
        10 => {
            check_str::<StreamId>(bytes);
            check_str::<ThreadId>(bytes);
            check_str::<BlobHash>(bytes);
        }
        11 => {
            check_str::<Title>(bytes);
            check_str::<AgentSessionId>(bytes);
            check_str::<AgentChoiceId>(bytes);
        }
        _ => {
            // JSON text of a RawPayload: what to_json_text writes must decode again.
            if let Some(raw) = check::<RawPayload>(bytes, msgpack)
                && let Ok(text) = raw.to_json_text()
            {
                let _: RawPayload = serde_json::from_str(&text)
                    .unwrap_or_else(|e| panic!("to_json_text output does not decode: {e}: {text}"));
            }
        }
    }
});
