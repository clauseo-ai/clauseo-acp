//! `RawPayload`: a value of any shape, kept as read so it can be written back as the same value.
//!
//! It is what an unknown event's payload, an elicitation schema and an unknown content block are
//! held as. It reads through `deserialize_any`, so it works the same from JSON (the wire) and from
//! `MessagePack` (`store`'s stored envelope), and writes back variant for variant, map entries in
//! their original order. `serde_json::Value` would sort map keys and refuse `MessagePack`'s
//! binary data; `serde_json`'s `RawValue` works only inside `serde_json`.
//!
//! Decoding never trusts a length a `MessagePack` header claims (it reserves at most 16 slots up
//! front). Decoding refuses values nested deeper than [`MAX_RAW_DEPTH`], and encoding values
//! nested deeper than [`MAX_RAW_ENCODE_DEPTH`], in both formats alike. Decoding refuses a map
//! key that is a list or a map, so a decoded value always has a JSON copy: NaN and a scalar or
//! binary map key come out in JSON's nearest form; see [`RawPayload`].

use std::borrow::Cow;
use std::fmt;

use serde::de::{self, Deserialize, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{self, Serialize, SerializeMap, SerializeSeq, Serializer};

use crate::Error;

/// A value of any shape, in the order it was read.
///
/// A `RawPayload` is always decoded from the frame's bytes (JSON or `MessagePack`), never
/// converted from a `serde_json::Value`: a `Value` built without `serde_json`'s
/// `preserve_order` feature has already sorted its keys.
///
/// Written back with the format's own encoding as the same value, map entries in their order.
/// JSON comes back as the same compact text for canonical input, except floats: without its
/// `float_roundtrip` feature `serde_json` may parse a float one ULP off, and `-0` comes back as
/// `-0.0`, so JSON floats come back within one ULP. `MessagePack` comes back byte for byte only
/// for what rmp-serde itself writes: canonical (shortest) integers and 64-bit floats. A wider
/// integer than needed comes back in the shortest form, and a 32-bit float comes back 64-bit.
///
/// Some values change form on the way, each one-way, and no Clauseo writer produces any of them:
/// - a `MessagePack` ext value decodes as `Seq([U64(type), Bytes(data)])` and writes back as
///   that array (`d4 05 01` as `92 05 c4 01 01`);
/// - a `MessagePack` str that is not valid UTF-8 decodes as `Bytes` and writes back as bin
///   (`a2 ff fe` as `c4 02 ff fe`);
/// - a JSON integer outside both `i64` and `u64` decodes as `F64` and writes back as a float
///   (`-18446744073709551617` as `-1.8446744073709552e+19`).
///
/// A `RawPayload` is exempt from [`MAX_SAFE`](crate::MAX_SAFE): a `U64` above 2^53 (an
/// elicitation schema's `maximum`, say) is written as its exact digits, and the webview, whose
/// numbers are 64-bit floats, reads it as an inexact number.
///
/// The JSON copy of a value read from `MessagePack` is a one-way projection where JSON has no
/// form for it. NaN and the infinities are written as `null`, which reads back as
/// [`RawPayload::Null`]. In a human-readable format a map key that is a scalar or binary data is
/// written as a string holding the key's compact JSON text: `7` as `"7"` (as `serde_json`
/// itself does), null as `"null"`, NaN as `"null"`, binary data as its list of byte numbers.
/// That text never holds a quote, so nothing is escaped twice. Such a key reads back as a string,
/// and it collides with the string key of the same text: a NaN or infinity key and `null` with
/// `"null"`, `true` and `false` with `"true"` and `"false"`, a number with its decimal string
/// (`7` and `"7"`, `1.5` and `"1.5"`), binary data with the string of its byte list (`[1,2]`
/// and `"[1,2]"`).
/// The copy then holds the key twice, and a JSON reader that keeps the last duplicate
/// (`serde_json::Value`, JS `JSON.parse`) silently drops the earlier entries.
///
/// Decoding refuses a map key that is a list or a map, in both formats (JSON cannot write one):
/// no Clauseo type keys a map that way (0002's freeze rules), so only a corrupt or hostile row
/// holds one, and it fails at the read as a too-deep one does. So every decoded value has a JSON
/// copy; only a value built by hand with such a key fails to encode in a human-readable format.
/// `MessagePack` writes every key as it is and keeps a scalar or binary key as read. A content
/// block's `raw` text is such a copy too.
///
/// Strings, binary data, lists and maps are boxed, since nothing grows them after they are read.
// Boxed, the enum is 24 bytes instead of 32.
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(feature = "specta", derive(specta::Type), specta(type = JsonValue))]
pub enum RawPayload {
    /// JSON `null`, `MessagePack` `nil`.
    Null,
    /// A boolean.
    Bool(bool),
    /// A negative integer (formats report non-negative ones as `U64`).
    I64(i64),
    /// A non-negative integer.
    U64(u64),
    /// A float. `MessagePack`'s 32-bit floats widen to this and write back as 64-bit.
    F64(f64),
    /// A string.
    Str(Box<str>),
    /// Binary data (`MessagePack` `bin`; JSON has none).
    Bytes(Box<[u8]>),
    /// A list.
    Seq(Box<[RawPayload]>),
    /// A map, entries in their original order; keys may repeat if the input repeated them.
    Map(Box<[(RawPayload, RawPayload)]>),
}

impl RawPayload {
    /// The value as compact JSON text, as `serde_json` writes it.
    ///
    /// Fails with [`Error::NotJson`] for a map key that is a list or a map (see [`RawPayload`]),
    /// which only a value built by hand can hold, and for a value nested deeper than
    /// [`MAX_RAW_ENCODE_DEPTH`].
    pub fn to_json_text(&self) -> Result<String, Error> {
        serde_json::to_string(self).map_err(|error| Error::NotJson(error.to_string()))
    }
}

/// Encoding refuses a value nested deeper than [`MAX_RAW_ENCODE_DEPTH`], 8 levels under what
/// decoding accepts, so a producer that built one fails when it commits, not when the log is
/// replayed, and a newer version that wraps the value in its own types still writes what this
/// version decodes.
impl Serialize for RawPayload {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AtDepth {
            value: self,
            depth: 0,
            limit: MAX_RAW_ENCODE_DEPTH,
        }
        .serialize(serializer)
    }
}

/// A value a catch-all decoded (an unknown event's payload, an unknown content block), written
/// back under the decode limit [`MAX_RAW_DEPTH`] instead of [`MAX_RAW_ENCODE_DEPTH`]: a newer
/// Clauseo may wrap its payloads in up to 8 levels of its own types, and what this version
/// decoded still reaches the webview, and a content block's `raw` text.
pub(crate) struct AsDecoded<'a>(pub(crate) &'a RawPayload);

impl Serialize for AsDecoded<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        AtDepth {
            value: self.0,
            depth: 0,
            limit: MAX_RAW_DEPTH,
        }
        .serialize(serializer)
    }
}

/// The depth one more list or map than `depth` opens; `None` past `limit`.
fn inner_depth(depth: usize, limit: usize) -> Option<usize> {
    Some(depth.saturating_add(1)).filter(|inner| *inner <= limit)
}

/// The error text for a value nested deeper than `limit` levels, encoding or decoding.
fn too_deep(limit: usize) -> String {
    format!("RawPayload: lists and maps nest deeper than {limit} levels")
}

/// A value to encode that sits inside `depth` lists and maps, of at most `limit`.
struct AtDepth<'a> {
    value: &'a RawPayload,
    depth: usize,
    limit: usize,
}

impl AtDepth<'_> {
    /// The value `value` one list or map down from this one, under the same limit.
    fn inner<'b>(&self, value: &'b RawPayload, depth: usize) -> AtDepth<'b> {
        AtDepth {
            value,
            depth,
            limit: self.limit,
        }
    }

    /// The depth of the items of a list or map this value opens; an error past `limit`,
    /// checked before the list or map opens, so an empty one past the limit fails too.
    fn items<E: ser::Error>(&self) -> Result<usize, E> {
        inner_depth(self.depth, self.limit).ok_or_else(|| E::custom(too_deep(self.limit)))
    }

    /// The key as a string for a human-readable format: itself when it is a string, its compact
    /// JSON text when it is a scalar or binary data (text with no quote in it, so nested keys
    /// are never escaped twice), and an error when it is a list or a map.
    fn key_text<E: ser::Error>(&self) -> Result<Cow<'_, str>, E> {
        match self.value {
            RawPayload::Str(text) => Ok(Cow::Borrowed(text)),
            RawPayload::Seq(_) | RawPayload::Map(_) => Err(E::custom(
                "RawPayload: a list or a map as a map key has no JSON form",
            )),
            RawPayload::Null
            | RawPayload::Bool(_)
            | RawPayload::I64(_)
            | RawPayload::U64(_)
            | RawPayload::F64(_)
            | RawPayload::Bytes(_) => serde_json::to_string(self)
                .map(Cow::Owned)
                .map_err(E::custom),
        }
    }
}

impl Serialize for AtDepth<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.value {
            RawPayload::Null => serializer.serialize_unit(),
            RawPayload::Bool(value) => serializer.serialize_bool(*value),
            RawPayload::I64(value) => serializer.serialize_i64(*value),
            RawPayload::U64(value) => serializer.serialize_u64(*value),
            RawPayload::F64(value) => serializer.serialize_f64(*value),
            RawPayload::Str(value) => serializer.serialize_str(value),
            RawPayload::Bytes(value) => serializer.serialize_bytes(value),
            RawPayload::Seq(items) => {
                let depth = self.items()?;
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for value in items {
                    seq.serialize_element(&self.inner(value, depth))?;
                }
                seq.end()
            }
            RawPayload::Map(entries) => {
                let depth = self.items()?;
                let text_keys = serializer.is_human_readable();
                let mut map = serializer.serialize_map(Some(entries.len()))?;
                for (key, value) in entries {
                    let value = self.inner(value, depth);
                    // A string key, the common case, goes straight through in every format.
                    if let RawPayload::Str(text) = key {
                        map.serialize_entry(&**text, &value)?;
                        continue;
                    }
                    let key = self.inner(key, depth);
                    if text_keys {
                        map.serialize_entry(&key.key_text::<S::Error>()?, &value)?;
                    } else {
                        map.serialize_entry(&key, &value)?;
                    }
                }
                map.end()
            }
        }
    }
}

/// The most lists and maps a [`RawPayload`] may nest when it is decoded, in JSON and in
/// `MessagePack` alike.
///
/// `serde_json` has its own limit of 127 levels, shared with the types around the payload (an
/// [`Envelope`](crate::Envelope), a `HostEvent`, a view entry), and rmp-serde allows 1024. A
/// limit of 100 leaves room for those types, so both formats accept the same values wherever a
/// `RawPayload` sits, and decoding stays well inside a debug build's 2 MiB test-thread stack.
///
/// Encoding stops 8 levels earlier, at [`MAX_RAW_ENCODE_DEPTH`]. A catch-all counts from its
/// payload's root, so every map a newer Clauseo wraps around a `RawPayload` (a new kind's
/// payload struct, a struct variant in a content block) costs a level here; the gap leaves room
/// for up to 8 such levels.
pub const MAX_RAW_DEPTH: usize = 100;

/// The most lists and maps a [`RawPayload`] may nest when it is encoded, in JSON and in
/// `MessagePack` alike: [`MAX_RAW_DEPTH`] less 8 levels of headroom (0002's freeze rules).
///
/// A producer that built a deeper value fails when it commits, not when the log is replayed,
/// and every Clauseo version encodes under this limit, so a newer version's `RawPayload`
/// still decodes here under up to 8 levels of types this version does not know. The one
/// exception is a value a catch-all decoded (an unknown event's payload, an unknown content
/// block), which writes back under [`MAX_RAW_DEPTH`], the limit it was read with.
pub const MAX_RAW_ENCODE_DEPTH: usize = MAX_RAW_DEPTH - 8;

/// The depth of the items of a list or map a decoded value at `depth` opens; an error past
/// [`MAX_RAW_DEPTH`].
fn decode_depth<E: de::Error>(depth: usize) -> Result<usize, E> {
    inner_depth(depth, MAX_RAW_DEPTH).ok_or_else(|| E::custom(too_deep(MAX_RAW_DEPTH)))
}

/// The nesting limit every reader of stored or received `MessagePack` sets on its decoder:
/// `rmp_serde::Deserializer::set_max_depth(MAX_MSGPACK_DEPTH)`.
///
/// [`MAX_RAW_DEPTH`] bounds only what decodes into a `RawPayload` or an open enum's catch-all.
/// A field an event struct does not know is skipped by serde, where nothing here counts it.
/// `serde_json` skips it without recursing, so any depth is harmless there. rmp-serde recurses
/// into it: plain `rmp_serde::from_slice` overflows a debug build's 2 MiB thread on about a
/// kilobyte of nested lists, so a reader that skips this setting can abort. 128 leaves room for
/// an [`Envelope`](crate::Envelope) around a payload at [`MAX_RAW_DEPTH`]. `protocol` itself
/// never reads `MessagePack`, so the reader (`store`, a transport) owns the setting.
///
/// A writer always uses `rmp_serde::to_vec_named`, never `rmp_serde::to_vec`: a struct payload
/// also decodes from an array by position, so `to_vec`'s arrays would silently swap fields.
pub const MAX_MSGPACK_DEPTH: usize = 128;

impl<'de> Deserialize<'de> for RawPayload {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        RawSeed { depth: 0 }.deserialize(deserializer)
    }
}

/// Reads one value that sits inside `depth` lists and maps.
#[derive(Clone, Copy)]
pub(crate) struct RawSeed {
    /// The lists and maps around the value.
    pub(crate) depth: usize,
}

impl<'de> DeserializeSeed<'de> for RawSeed {
    type Value = RawPayload;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<RawPayload, D::Error> {
        deserializer.deserialize_any(RawVisitor { depth: self.depth })
    }
}

/// The slots to reserve for a list or map whose format gave `hint` as its length: at most 16.
///
/// A `MessagePack` header's length is only a claim (rmp-serde passes it on as the hint, and a
/// 5-byte array32 header may claim 2^32 - 1 items), so it is never trusted; past 16 the `Vec`
/// grows as items actually arrive.
fn reserve(hint: Option<usize>) -> usize {
    hint.unwrap_or(0).min(16)
}

/// The decode error for a map key that is a list or a map.
const LIST_OR_MAP_KEY: &str = "RawPayload: a list or a map as a map key";

struct RawVisitor {
    /// The lists and maps around the value.
    depth: usize,
}

impl RawVisitor {
    /// The seed for the items of a list or map this value opens; an error past [`MAX_RAW_DEPTH`].
    fn items<E: de::Error>(&self) -> Result<RawSeed, E> {
        let depth = decode_depth(self.depth)?;
        Ok(RawSeed { depth })
    }
}

impl<'de> Visitor<'de> for RawVisitor {
    type Value = RawPayload;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any value")
    }

    fn visit_unit<E: de::Error>(self) -> Result<RawPayload, E> {
        Ok(RawPayload::Null)
    }

    fn visit_none<E: de::Error>(self) -> Result<RawPayload, E> {
        Ok(RawPayload::Null)
    }

    fn visit_some<D: Deserializer<'de>>(self, deserializer: D) -> Result<RawPayload, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<RawPayload, D::Error> {
        deserializer.deserialize_any(self)
    }

    fn visit_bool<E: de::Error>(self, v: bool) -> Result<RawPayload, E> {
        Ok(RawPayload::Bool(v))
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<RawPayload, E> {
        // A format that reports a non-negative number as signed still gets one form per number.
        Ok(u64::try_from(v).map_or(RawPayload::I64(v), RawPayload::U64))
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<RawPayload, E> {
        Ok(RawPayload::U64(v))
    }

    fn visit_f64<E: de::Error>(self, v: f64) -> Result<RawPayload, E> {
        Ok(RawPayload::F64(v))
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<RawPayload, E> {
        Ok(RawPayload::Str(v.into()))
    }

    fn visit_string<E: de::Error>(self, v: String) -> Result<RawPayload, E> {
        Ok(RawPayload::Str(v.into_boxed_str()))
    }

    fn visit_bytes<E: de::Error>(self, v: &[u8]) -> Result<RawPayload, E> {
        Ok(RawPayload::Bytes(v.into()))
    }

    fn visit_byte_buf<E: de::Error>(self, v: Vec<u8>) -> Result<RawPayload, E> {
        Ok(RawPayload::Bytes(v.into_boxed_slice()))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<RawPayload, A::Error> {
        let seed = self.items()?;
        let mut items = Vec::with_capacity(reserve(seq.size_hint()));
        while let Some(item) = seq.next_element_seed(seed)? {
            items.push(item);
        }
        Ok(RawPayload::Seq(items.into_boxed_slice()))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RawPayload, A::Error> {
        let seed = self.items()?;
        let mut entries = Vec::with_capacity(reserve(map.size_hint()));
        while let Some(key) = map.next_key_seed(seed)? {
            // No Clauseo type writes such a key (0002's freeze rules) and it has no JSON form,
            // so a row holding one is corrupt and fails here, not when a batch is sent.
            if matches!(key, RawPayload::Seq(_) | RawPayload::Map(_)) {
                return Err(de::Error::custom(LIST_OR_MAP_KEY));
            }
            entries.push((key, map.next_value_seed(seed)?));
        }
        Ok(RawPayload::Map(entries.into_boxed_slice()))
    }
}

/// The TypeScript shape of a [`RawPayload`]: any JSON value. specta has no `unknown` outside
/// specta-typescript, which this crate may not depend on, so the union says the same thing.
#[cfg(feature = "specta")]
#[derive(serde::Serialize, specta::Type)]
#[serde(untagged)]
#[expect(
    dead_code,
    reason = "only its specta::Type impl is used, to describe RawPayload"
)]
enum JsonValue {
    // `()` renders `null`; a unit variant would render as the string "Null".
    Null(()),
    Bool(bool),
    // `i32` renders `number`; `f64` would add a second `| null`.
    Number(i32),
    Text(String),
    List(Vec<JsonValue>),
    Object(std::collections::BTreeMap<String, JsonValue>),
}

mod skip;

pub(crate) use skip::SkipSeed;
