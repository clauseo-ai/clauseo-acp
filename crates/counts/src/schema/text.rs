//! Serde for the closed types that travel as a string of one fixed form (a day, a week, a
//! version, an id, an error code): written with `Display`, read only through the type's strict
//! `FromStr`, so a string in any other form is refused.

use std::fmt;
use std::marker::PhantomData;
use std::str::FromStr;

use serde::de::{self, Visitor};

/// Reads a string and parses it with `T`'s `FromStr`; any other JSON value is refused.
pub(crate) struct FromStrVisitor<T>(PhantomData<T>);

impl<T> FromStrVisitor<T> {
    pub(crate) const fn new() -> Self {
        Self(PhantomData)
    }
}

impl<T: FromStr> Visitor<'_> for FromStrVisitor<T>
where
    T::Err: fmt::Display,
{
    type Value = T;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string in the schema's form")
    }

    fn visit_str<E: de::Error>(self, v: &str) -> Result<T, E> {
        v.parse().map_err(E::custom)
    }
}

/// `Serialize` through `Display`, `Deserialize` through `FromStr`, the seal, and (in tests) a
/// JSON Schema of a string with `$pattern`, or `$schema` when given whole.
macro_rules! as_text {
    ($name:ident, $schema_name:literal, $pattern:expr) => {
        $crate::schema::text::as_text!(
            $name,
            $schema_name,
            schema: schemars::json_schema!({ "type": "string", "pattern": $pattern })
        );
    };
    ($name:ident, $schema_name:literal, schema: $schema:expr) => {
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.collect_str(self)
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                deserializer.deserialize_str($crate::schema::text::FromStrVisitor::new())
            }
        }

        impl $crate::schema::closed::sealed::Sealed for $name {}
        impl $crate::schema::Closed for $name {}

        #[cfg(test)]
        impl schemars::JsonSchema for $name {
            fn schema_name() -> std::borrow::Cow<'static, str> {
                $schema_name.into()
            }

            fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
                $schema
            }
        }
    };
}

pub(crate) use as_text;
