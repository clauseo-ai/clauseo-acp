//! What the stdout line filter reads of a line ([`Envelope`]), decoded without a `Value` and
//! refusing every line the SDK's `Value` decode refuses or reads differently ([`Checked`]).

use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

/// What the filter reads of a line, with no `Value` and no allocation: which JSON-RPC keys it
/// has, and the shapes [`Envelope::is_update`] needs. Only a JSON object decodes: a number, a
/// string or an array (ACP sends no batches) fails like a line that is not JSON, and so does a
/// line the SDK's `Value` decode refuses ([`Checked`]). A key that appears twice counts as its
/// last value, as in the SDK's own decode.
#[derive(Debug, Default)]
pub(super) struct Envelope {
    jsonrpc: Option<Shape>,
    method: Option<Shape>,
    params: Option<Shape>,
    id: bool,
    result: bool,
    error: bool,
}

impl Envelope {
    /// A JSON-RPC message as the SDK's own decode reads one (`RawJsonRpcMessage`): `jsonrpc` is
    /// `"2.0"`, and there is a `method` with no `result` or `error` (a request or a
    /// notification), or an `id` with exactly one of `result` and `error` and no `method` (a
    /// response). Anything else the SDK would answer with an error, and a log line an adapter
    /// wrote to stdout is not one.
    pub(super) fn is_message(&self) -> bool {
        let request = self.method.is_some() && !self.result && !self.error;
        let response = self.method.is_none() && self.id && self.result != self.error;
        self.jsonrpc == Some(Shape::Text { matches: true }) && (request || response)
    }

    /// A `session/update` notification the SDK decodes and hands the inbound handler: `jsonrpc`
    /// is `"2.0"`, `params` an object, and there is no `id`, `result` or `error`.
    pub(super) fn is_update(&self) -> bool {
        self.jsonrpc == Some(Shape::Text { matches: true })
            && self.method == Some(Shape::Text { matches: true })
            && self.params == Some(Shape::Object)
            && !(self.id || self.result || self.error)
    }
}

impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(EnvelopeVisitor)
    }
}

struct EnvelopeVisitor;

impl<'de> Visitor<'de> for EnvelopeVisitor {
    type Value = Envelope;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a JSON-RPC message")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Envelope, A::Error> {
        let mut envelope = Envelope::default();
        while let Some(key) = map.next_key::<Key>()? {
            match key {
                Key::Jsonrpc => envelope.jsonrpc = Some(map.next_value_seed(ShapeOf("2.0"))?),
                Key::Method => {
                    envelope.method = Some(map.next_value_seed(ShapeOf("session/update"))?);
                }
                Key::Params => envelope.params = Some(map.next_value_seed(ShapeOf(""))?),
                Key::Id => envelope.id = map.next_value_seed(Checked).map(|()| true)?,
                Key::Result => envelope.result = map.next_value_seed(Checked).map(|()| true)?,
                Key::Error => envelope.error = map.next_value_seed(Checked).map(|()| true)?,
                Key::Other => map.next_value_seed(Checked)?,
            }
        }
        Ok(envelope)
    }
}

/// A key of a JSON-RPC message, read without copying it.
#[derive(Clone, Copy)]
enum Key {
    Jsonrpc,
    Method,
    Params,
    Id,
    Result,
    Error,
    Other,
}

impl<'de> Deserialize<'de> for Key {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(KeyVisitor)
    }
}

struct KeyVisitor;

impl Visitor<'_> for KeyVisitor {
    type Value = Key;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a key")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Key, E> {
        Ok(match v {
            "jsonrpc" => Key::Jsonrpc,
            "method" => Key::Method,
            "params" => Key::Params,
            "id" => Key::Id,
            "result" => Key::Result,
            "error" => Key::Error,
            _ => return CheckedKey.visit_str(v).map(|()| Key::Other),
        })
    }
}

/// `serde_json`'s private key for a `RawValue` (`serde_json::raw::TOKEN`, not public). `serde_json`
/// is built with `raw_value`, so its `Value` decode reads a map whose first key is this as the
/// JSON text inside its string value: it refuses the line when that text is not JSON, and reads
/// a different value (an array where an object was, say) when it is.
const RAW_VALUE_TOKEN: &str = "$serde_json::private::RawValue";

/// Walks a value and keeps nothing, refusing what the SDK's `Value` decode refuses or reads
/// differently. `IgnoredAny` would skip it without reading it: `serde_json` skips a number or a
/// string escape by its characters alone and has no depth limit there, so a number out of
/// `f64`'s range (`1e400`), a lone surrogate (`"\ud800"`) or nesting deeper than `serde_json`'s
/// limit (127 levels) would pass here and fail in the SDK. `deserialize_any` decodes each number
/// and string and counts depth as `Value` does, and [`CheckedKey`] refuses [`RAW_VALUE_TOKEN`] as
/// a key.
struct Checked;

impl<'de> DeserializeSeed<'de> for Checked {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for Checked {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any value")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while map.next_entry_seed(CheckedKey, Checked)?.is_some() {}
        Ok(())
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
        while seq.next_element_seed(Checked)?.is_some() {}
        Ok(())
    }

    fn visit_str<E: de::Error>(self, _: &str) -> Result<(), E> {
        Ok(())
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<(), E> {
        Ok(())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<(), E> {
        Ok(())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<(), E> {
        Ok(())
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<(), E> {
        Ok(())
    }

    fn visit_unit<E: de::Error>(self) -> Result<(), E> {
        Ok(())
    }
}

/// Reads a key without copying it, refusing [`RAW_VALUE_TOKEN`] in any place, not only the first
/// (no agent sends it).
struct CheckedKey;

impl<'de> DeserializeSeed<'de> for CheckedKey {
    type Value = ();

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_str(self)
    }
}

impl Visitor<'_> for CheckedKey {
    type Value = ();

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a key")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<(), E> {
        if v == RAW_VALUE_TOKEN {
            Err(E::custom("serde_json's private RawValue key"))
        } else {
            Ok(())
        }
    }
}

/// The kind of one value, read without keeping it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    /// A string; `matches` when it is the one [`ShapeOf`] looks for.
    Text { matches: bool },
    /// An object.
    Object,
    /// Anything else.
    Other,
}

/// Reads a value's [`Shape`], comparing a string with the text it holds.
struct ShapeOf(&'static str);

impl<'de> DeserializeSeed<'de> for ShapeOf {
    type Value = Shape;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<Shape, D::Error> {
        deserializer.deserialize_any(self)
    }
}

impl<'de> Visitor<'de> for ShapeOf {
    type Value = Shape;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any value")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Shape, E> {
        Ok(Shape::Text {
            matches: v == self.0,
        })
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<Shape, A::Error> {
        Checked.visit_map(map).map(|()| Shape::Object)
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Shape, A::Error> {
        Checked.visit_seq(seq).map(|()| Shape::Other)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Shape, E> {
        Ok(Shape::Other)
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Shape, E> {
        Ok(Shape::Other)
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Shape, E> {
        Ok(Shape::Other)
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Shape, E> {
        Ok(Shape::Other)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Shape, E> {
        Ok(Shape::Other)
    }
}
