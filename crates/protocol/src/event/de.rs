//! `DomainEvent`'s hand-written `Deserialize`, in one pass.
//!
//! The visitor reads the one-key object's key as a borrowed `&str`, matches it into the private
//! `Kind`, then calls `next_value_seed` with a seed for that kind, which decodes the payload
//! straight into its type. The payload is never buffered and parsed a second time; only an
//! unknown kind is read as a [`RawPayload`].

use std::fmt;

use serde::Deserialize;
use serde::de::{self, DeserializeSeed, Deserializer, IgnoredAny, MapAccess, Visitor};

use super::{DomainEvent, Kind};
use crate::RawPayload;
use crate::open_enum::UnknownKind;
use crate::serde_util::BorrowedStr;

impl<'de> Deserialize<'de> for DomainEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_map(EventVisitor)
    }
}

struct EventVisitor;

impl<'de> Visitor<'de> for EventVisitor {
    type Value = DomainEvent;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a domain event: a map with one key, the kind")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<DomainEvent, A::Error> {
        let Some(key) = map.next_key::<BorrowedStr<'de>>()? else {
            return Err(de::Error::invalid_length(0, &self));
        };
        let event = match Kind::from_key(key.as_str()) {
            Some(kind) => map.next_value_seed(PayloadSeed(kind))?,
            None => DomainEvent::Unknown {
                kind: UnknownKind::new(key.as_str()),
                payload: map.next_value::<RawPayload>()?,
            },
        };
        if map.next_key::<IgnoredAny>()?.is_some() {
            return Err(de::Error::invalid_length(2, &self));
        }
        Ok(event)
    }
}

/// Decodes the payload of one known kind.
struct PayloadSeed(Kind);

impl<'de> DeserializeSeed<'de> for PayloadSeed {
    type Value = DomainEvent;

    fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<DomainEvent, D::Error> {
        self.0.decode(deserializer)
    }
}
