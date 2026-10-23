//! `Envelope`'s and `EnvelopeRead`'s hand-written `Deserialize`, in one pass.
//!
//! The visitor reads the map's keys in order, each as a [`Field`]. `schema_version` decides how
//! `event` is read. Serde writes it first, so an `event` after it is decoded in place, never
//! buffered. An `event` before it (keys sorted, as a `serde_json::Value` or `jq -S` does) is read
//! by an out-of-line function that takes over the rest of the map: it holds the event with
//! [`HeldSeed`], under the format's own nesting limit only, and decodes it through
//! [`RawDeserializer`] once the version is known, at [`SCHEMA_VERSION`] as a [`DomainEvent`],
//! older as the [`RawPayload`] [`upcast`] takes; the in-order loop keeps no state for it. A
//! newer version stops the decode: the rest of the map is skipped as [`IgnoredAny`] and the value
//! is [`EnvelopeRead::Newer`], or an error for an [`Envelope`]. The other fields may come in any
//! order around them, and a field this version does not know is skipped; a repeated field is
//! refused before its second value is read. One visitor body serves both types (generic over
//! [`Output`]), so an `Envelope` is built directly, never moved out of an `EnvelopeRead`; the
//! newer, older and event-first paths are out of line, as rare.

use std::fmt;
use std::marker::PhantomData;

use serde::Deserialize;
use serde::de::{self, Deserializer, IgnoredAny, MapAccess, Visitor};

use super::held::HeldSeed;
use super::raw_de::RawDeserializer;
use super::{Envelope, EnvelopeRead, upcast};
use crate::{
    CommandId, DomainEvent, EventId, RawPayload, SCHEMA_VERSION, Seq, StreamId, Timestamp,
};

impl<'de> Deserialize<'de> for EnvelopeRead {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        visit(deserializer)
    }
}

impl<'de> Deserialize<'de> for Envelope {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        visit(deserializer)
    }
}

/// Decodes `T` from a map; the format's `is_human_readable` (a constant in `serde_json` and
/// `rmp-serde`) picks the visitor, so the loop itself never tests it.
fn visit<'de, T: Output, D: Deserializer<'de>>(deserializer: D) -> Result<T, D::Error> {
    if deserializer.is_human_readable() {
        deserializer.deserialize_map(EnvelopeVisitor::<T, true>(PhantomData))
    } else {
        deserializer.deserialize_map(EnvelopeVisitor::<T, false>(PhantomData))
    }
}

/// What the visitor builds from a decoded envelope or a newer version.
trait Output: Sized {
    /// The value for an envelope at [`SCHEMA_VERSION`] (or upcast to it).
    fn current(envelope: Envelope) -> Self;
    /// The value for an envelope written at `schema_version`, newer than [`SCHEMA_VERSION`].
    fn newer<E: de::Error>(schema_version: u32) -> Result<Self, E>;
}

impl Output for EnvelopeRead {
    fn current(envelope: Envelope) -> Self {
        Self::Current(envelope)
    }

    fn newer<E: de::Error>(schema_version: u32) -> Result<Self, E> {
        Ok(Self::Newer { schema_version })
    }
}

impl Output for Envelope {
    fn current(envelope: Envelope) -> Self {
        envelope
    }

    fn newer<E: de::Error>(schema_version: u32) -> Result<Self, E> {
        Err(E::custom(format_args!(
            "Envelope: schema_version {schema_version} is newer than {SCHEMA_VERSION}; \
             decode an EnvelopeRead"
        )))
    }
}

/// An envelope's map key: one of its fields, or one this version does not know.
enum Field {
    SchemaVersion,
    EventId,
    Stream,
    Seq,
    Ts,
    CommandId,
    Event,
    Other,
}

impl<'de> Deserialize<'de> for Field {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_identifier(FieldVisitor)
    }
}

struct FieldVisitor;

impl Visitor<'_> for FieldVisitor {
    type Value = Field;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an envelope field name")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<Field, E> {
        Ok(match v {
            "schema_version" => Field::SchemaVersion,
            "event_id" => Field::EventId,
            "stream" => Field::Stream,
            "seq" => Field::Seq,
            "ts" => Field::Ts,
            "command_id" => Field::CommandId,
            "event" => Field::Event,
            _ => Field::Other,
        })
    }
}

/// The visitor; `HUMAN` is the format's `is_human_readable`, which only an `event` read before
/// `schema_version` needs (a constant, so the in-order loop carries nothing for it).
struct EnvelopeVisitor<T, const HUMAN: bool>(PhantomData<T>);

impl<'de, T: Output, const HUMAN: bool> Visitor<'de> for EnvelopeVisitor<T, HUMAN> {
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("an envelope: a map of its fields")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<T, A::Error> {
        let mut version = None;
        let mut event_id = None;
        let mut stream = None;
        let mut seq = None;
        let mut ts = None;
        let mut command_id: Option<Option<CommandId>> = None;
        let mut event = None;
        while let Some(key) = map.next_key::<Field>()? {
            match key {
                Field::SchemaVersion => {
                    unset(version.as_ref(), "schema_version")?;
                    let found = map.next_value::<u32>()?;
                    version = Some(found);
                    if found > SCHEMA_VERSION {
                        return newer(map, found);
                    }
                }
                Field::Event => {
                    let Some(found) = version else {
                        let before = Before {
                            event_id,
                            stream,
                            seq,
                            ts,
                            command_id,
                        };
                        return event_first::<T, A, HUMAN>(map, before);
                    };
                    unset(event.as_ref(), "event")?;
                    event = Some(if found == SCHEMA_VERSION {
                        map.next_value::<DomainEvent>()?
                    } else {
                        older(&mut map, found)?
                    });
                }
                Field::EventId => {
                    unset(event_id.as_ref(), "event_id")?;
                    event_id = Some(map.next_value()?);
                }
                Field::Stream => {
                    unset(stream.as_ref(), "stream")?;
                    stream = Some(map.next_value()?);
                }
                Field::Seq => {
                    unset(seq.as_ref(), "seq")?;
                    seq = Some(map.next_value()?);
                }
                Field::Ts => {
                    unset(ts.as_ref(), "ts")?;
                    ts = Some(map.next_value()?);
                }
                Field::CommandId => {
                    unset(command_id.as_ref(), "command_id")?;
                    command_id = Some(map.next_value()?);
                }
                Field::Other => {
                    map.next_value::<IgnoredAny>()?;
                }
            }
        }
        // An event is present only after a version at most this one, upcast if older; an
        // `event` before `schema_version` went to `event_first`.
        Ok(T::current(Envelope {
            schema_version: SCHEMA_VERSION,
            event_id: event_id.ok_or_else(|| de::Error::missing_field("event_id"))?,
            stream: stream.ok_or_else(|| de::Error::missing_field("stream"))?,
            seq: seq.ok_or_else(|| de::Error::missing_field("seq"))?,
            ts: ts.ok_or_else(|| de::Error::missing_field("ts"))?,
            command_id: command_id.flatten(),
            event: event.ok_or_else(|| de::Error::missing_field("event"))?,
        }))
    }
}

/// Skips the rest of the map and gives `T`'s value for a newer version; out of line, as rare.
#[cold]
#[inline(never)]
fn newer<'de, T: Output, A: MapAccess<'de>>(mut map: A, found: u32) -> Result<T, A::Error> {
    skip_rest(&mut map)?;
    T::newer(found)
}

/// Reads an event written at `found`, older than this version, through [`upcast`]; out of line.
#[cold]
#[inline(never)]
fn older<'de, A: MapAccess<'de>>(map: &mut A, found: u32) -> Result<DomainEvent, A::Error> {
    upcast(found, map.next_value::<RawPayload>()?).map_err(de::Error::custom)
}

/// The fields read before an `event` that came before `schema_version`; `C` is
/// `Option<CommandId>`, `command_id` being `Some` once read (null or not).
struct Before<C> {
    event_id: Option<EventId>,
    stream: Option<StreamId>,
    seq: Option<Seq>,
    ts: Option<Timestamp>,
    command_id: Option<C>,
}

/// Takes over the map at an `event` key that came before `schema_version` (keys sorted), out of
/// line, as rare: holds the event with [`HeldSeed`] (any value the format reads), reads the rest
/// of the map, and decodes the event once the version is known through [`RawDeserializer`] (as a
/// format whose `is_human_readable` is `HUMAN`): as this version's [`DomainEvent`], or if older
/// as the [`RawPayload`] [`upcast`] takes, so [`MAX_RAW_DEPTH`](crate::MAX_RAW_DEPTH) and the
/// map-key rule apply there as in order. A newer version skips the rest as in order, whatever the
/// held event holds. `before` holds the fields read so far.
#[cold]
#[inline(never)]
fn event_first<'de, T: Output, A: MapAccess<'de>, const HUMAN: bool>(
    mut map: A,
    Before {
        mut event_id,
        mut stream,
        mut seq,
        mut ts,
        mut command_id,
    }: Before<Option<CommandId>>,
) -> Result<T, A::Error> {
    let payload = map.next_value_seed(HeldSeed)?;
    let mut version = None;
    while let Some(key) = map.next_key::<Field>()? {
        match key {
            Field::SchemaVersion => {
                unset(version.as_ref(), "schema_version")?;
                let found = map.next_value::<u32>()?;
                if found > SCHEMA_VERSION {
                    return newer(map, found);
                }
                version = Some(found);
            }
            Field::Event => return Err(de::Error::duplicate_field("event")),
            Field::EventId => {
                unset(event_id.as_ref(), "event_id")?;
                event_id = Some(map.next_value()?);
            }
            Field::Stream => {
                unset(stream.as_ref(), "stream")?;
                stream = Some(map.next_value()?);
            }
            Field::Seq => {
                unset(seq.as_ref(), "seq")?;
                seq = Some(map.next_value()?);
            }
            Field::Ts => {
                unset(ts.as_ref(), "ts")?;
                ts = Some(map.next_value()?);
            }
            Field::CommandId => {
                unset(command_id.as_ref(), "command_id")?;
                command_id = Some(map.next_value()?);
            }
            Field::Other => {
                map.next_value::<IgnoredAny>()?;
            }
        }
    }
    let found = version.ok_or_else(|| de::Error::missing_field("schema_version"))?;
    let event = if found == SCHEMA_VERSION {
        DomainEvent::deserialize(RawDeserializer::new(&payload, HUMAN))?
    } else {
        let payload = RawPayload::deserialize(RawDeserializer::new(&payload, HUMAN))?;
        upcast(found, payload).map_err(de::Error::custom)?
    };
    Ok(T::current(Envelope {
        schema_version: SCHEMA_VERSION,
        event_id: event_id.ok_or_else(|| de::Error::missing_field("event_id"))?,
        stream: stream.ok_or_else(|| de::Error::missing_field("stream"))?,
        seq: seq.ok_or_else(|| de::Error::missing_field("seq"))?,
        ts: ts.ok_or_else(|| de::Error::missing_field("ts"))?,
        command_id: command_id.flatten(),
        event,
    }))
}

/// Fails if `field` was already read into `slot`; checked before its value is read.
fn unset<T, E: de::Error>(slot: Option<&T>, field: &'static str) -> Result<(), E> {
    if slot.is_some() {
        return Err(E::duplicate_field(field));
    }
    Ok(())
}

/// Reads and drops the map's remaining entries, so the format sees the whole map consumed.
fn skip_rest<'de, A: MapAccess<'de>>(map: &mut A) -> Result<(), A::Error> {
    while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
    Ok(())
}
