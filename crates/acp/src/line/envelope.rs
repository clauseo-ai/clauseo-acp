//! What the stdout line filter reads of a line ([`Envelope`]), decoded without a `Value`,
//! refusing every line the SDK's decode refuses or reads differently: its `Value` decode
//! ([`Checked`]) and then its `RawJsonRpcMessage` decode ([`Envelope::is_message`]).

use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde::{Deserialize, Deserializer};

use super::MALFORMED_LINE_METHOD;

/// What the filter reads of a line, with no `Value` (escaped strings go through `serde_json`'s
/// scratch buffer): which JSON-RPC keys it has, and the shapes of their values. Only a JSON
/// object decodes: a number, a string or an array (ACP sends no batches) fails like a line that
/// is not JSON, and so does a line the SDK's `Value` decode refuses ([`Checked`]). A key that
/// appears twice counts as its last value, as in the SDK's own decode, which reads a `Value`
/// first.
#[derive(Debug, Default)]
pub(super) struct Envelope {
    jsonrpc: Option<Shape>,
    method: Option<Shape>,
    params: Option<Shape>,
    id: Option<Shape>,
    result: bool,
    error: Option<Shape>,
}

impl Envelope {
    /// A JSON-RPC message as the SDK's own decode reads one (`RawJsonRpcMessage` in
    /// agent-client-protocol 3.3.0, over the schema crate's `rpc` types): `jsonrpc` is `"2.0"`,
    /// and either
    /// - there is a `method` with no `result` or `error` (a request when it has an `id`, a
    ///   notification when not): `method` is a string, `params` is absent, `null`, an object or
    ///   an array, and the `id` if any is a `RequestId` (`null`, a string or an `i64`); or
    /// - there is no `method`, a `RequestId` `id`, and exactly one of `result` (any value) and
    ///   `error` (an object whose `code` is an `i32` and whose `message` is a string): a response.
    ///
    /// Anything else the SDK answers with an error, and a log line an adapter wrote to stdout is
    /// not one. Nor is a line naming [`MALFORMED_LINE_METHOD`]: only the filter's own marker may
    /// (the inbound handler counts it off the gate and turns it into a warning).
    pub(super) fn is_message(&self) -> bool {
        if self.jsonrpc != Some(Shape::Text(Word::Version)) {
            return false;
        }
        let id_fits = |id: Option<Shape>| id.is_some_and(Shape::is_request_id);
        if self.method.is_some() && !self.result && self.error.is_none() {
            matches!(self.method, Some(Shape::Text(word)) if word != Word::Marker)
                && matches!(
                    self.params,
                    None | Some(Shape::Null | Shape::Object { .. } | Shape::Array)
                )
                && (self.id.is_none() || id_fits(self.id))
        } else if self.method.is_none() && self.result != self.error.is_some() {
            id_fits(self.id)
                && (self.result || self.error == Some(Shape::Object { is_error: true }))
        } else {
            false
        }
    }

    /// A `session/update` notification the SDK decodes and hands the inbound handler: a message
    /// ([`Envelope::is_message`]) whose `params` is an object, with no `id`, `result` or `error`.
    pub(super) fn is_update(&self) -> bool {
        self.is_message()
            && self.method == Some(Shape::Text(Word::Update))
            && matches!(self.params, Some(Shape::Object { .. }))
            && self.id.is_none()
            && !self.result
            && self.error.is_none()
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
                Key::Jsonrpc => envelope.jsonrpc = Some(map.next_value_seed(ShapeOf::VALUE)?),
                Key::Method => envelope.method = Some(map.next_value_seed(ShapeOf::VALUE)?),
                Key::Params => envelope.params = Some(map.next_value_seed(ShapeOf::VALUE)?),
                Key::Id => envelope.id = Some(map.next_value_seed(ShapeOf::VALUE)?),
                Key::Result => envelope.result = map.next_value_seed(Checked).map(|()| true)?,
                Key::Error => envelope.error = Some(map.next_value_seed(ShapeOf::ERROR)?),
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
            _ => {
                CheckedKey.visit_str(v)?;
                Key::Other
            }
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
/// limit (127 levels) would pass here and fail in the SDK. `deserialize_any` decodes the value
/// fully, counting depth as `Value` does, and calls the matching `visit_*` below; each returns
/// `()`, so nothing is kept. [`CheckedKey`] refuses [`RAW_VALUE_TOKEN`] as a key.
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
    /// A string, and which of the words the filter looks for it is.
    Text(Word),
    /// An integer, and whether it fits the SDK's `i64` id and `i32` error code.
    Integer { i64: bool, i32: bool },
    /// An object; `is_error` when its `code` is an `i32` and its `message` a string, as the
    /// SDK's `RawJsonRpcError` needs.
    Object { is_error: bool },
    /// An array.
    Array,
    /// `null`.
    Null,
    /// A boolean or a number with a fraction or an exponent.
    Other,
}

impl Shape {
    /// A `RequestId`: `null`, a string or an `i64`.
    fn is_request_id(self) -> bool {
        matches!(
            self,
            Self::Null | Self::Text(_) | Self::Integer { i64: true, .. }
        )
    }
}

/// The strings the filter tells apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Word {
    /// `"2.0"`.
    Version,
    /// `"session/update"`.
    Update,
    /// [`MALFORMED_LINE_METHOD`].
    Marker,
    /// Any other string.
    Other,
}

/// Reads a value's [`Shape`], walking what it does not keep with [`Checked`]. Only for `error`
/// does it read an object's `code` and `message` ([`Shape::Object`]'s `is_error`).
struct ShapeOf {
    error: bool,
}

impl ShapeOf {
    /// Any value but `error`.
    const VALUE: Self = Self { error: false };
    /// The value of `error`.
    const ERROR: Self = Self { error: true };
}

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
        Ok(Shape::Text(match v {
            "2.0" => Word::Version,
            "session/update" => Word::Update,
            MALFORMED_LINE_METHOD => Word::Marker,
            _ => Word::Other,
        }))
    }

    /// For `error`, reads `code` and `message` one level down, last value wins; everything else
    /// is walked.
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Shape, A::Error> {
        if !self.error {
            return Checked
                .visit_map(map)
                .map(|()| Shape::Object { is_error: false });
        }
        let (mut code, mut message) = (None, None);
        while let Some(key) = map.next_key::<ErrorKey>()? {
            match key {
                ErrorKey::Code => code = Some(map.next_value_seed(ShapeOf::VALUE)?),
                ErrorKey::Message => message = Some(map.next_value_seed(ShapeOf::VALUE)?),
                ErrorKey::Other => map.next_value_seed(Checked)?,
            }
        }
        Ok(Shape::Object {
            is_error: matches!(code, Some(Shape::Integer { i32: true, .. }))
                && matches!(message, Some(Shape::Text(_))),
        })
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<Shape, A::Error> {
        Checked.visit_seq(seq).map(|()| Shape::Array)
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Shape, E> {
        Ok(Shape::Other)
    }

    fn visit_i64<E: de::Error>(self, v: i64) -> Result<Shape, E> {
        Ok(Shape::Integer {
            i64: true,
            i32: i32::try_from(v).is_ok(),
        })
    }

    fn visit_u64<E: de::Error>(self, v: u64) -> Result<Shape, E> {
        Ok(Shape::Integer {
            i64: i64::try_from(v).is_ok(),
            i32: i32::try_from(v).is_ok(),
        })
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Shape, E> {
        Ok(Shape::Other)
    }

    fn visit_unit<E: de::Error>(self) -> Result<Shape, E> {
        Ok(Shape::Null)
    }
}

/// A key of an `error` object, read without copying it.
#[derive(Clone, Copy)]
enum ErrorKey {
    Code,
    Message,
    Other,
}

impl<'de> Deserialize<'de> for ErrorKey {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(ErrorKeyVisitor)
    }
}

struct ErrorKeyVisitor;

impl Visitor<'_> for ErrorKeyVisitor {
    type Value = ErrorKey;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a key")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<ErrorKey, E> {
        Ok(match v {
            "code" => ErrorKey::Code,
            "message" => ErrorKey::Message,
            _ => {
                CheckedKey.visit_str(v)?;
                ErrorKey::Other
            }
        })
    }
}
