//! 64-bit numbers on the wire and in the log: every one stays at most [`MAX_SAFE`] (2^53 - 1), so
//! TypeScript reads it as an exact `number`.
//!
//! A plain `u64` field of a wire or stored type carries `#[serde(with = "crate::number::safe")]`
//! (`crate::number::safe_opt` for an `Option<u64>`). Both encoding and decoding refuse a larger
//! value, so a producer bug fails when the event is committed instead of writing a log entry no
//! reader can decode. The JSON and `MessagePack` forms are those of a plain `u64`. Producers that
//! pass on counts an agent reported (R10) cap them first with [`clamp_safe`].

use crate::MAX_SAFE;

/// `value`, or [`MAX_SAFE`] when it is larger: for counts an agent reports, which Clauseo passes
/// on but does not trust.
pub const fn clamp_safe(value: u64) -> u64 {
    if value > MAX_SAFE { MAX_SAFE } else { value }
}

/// The error text for a number above [`MAX_SAFE`].
fn too_large(value: u64) -> String {
    format!("{value} is above 2^53 - 1, the largest number this protocol carries")
}

/// `#[serde(with)]` for a `u64` field that must stay at most [`MAX_SAFE`].
pub(crate) mod safe {
    use serde::{Deserialize, Deserializer, Serializer, de, ser};

    use super::too_large;
    use crate::MAX_SAFE;

    /// Writes `value` as a `u64`; an error when it is above [`MAX_SAFE`]. (Generic only because
    /// serde passes the field by reference.)
    pub(crate) fn serialize<V, S>(value: &V, serializer: S) -> Result<S::Ok, S::Error>
    where
        V: Copy + Into<u64>,
        S: Serializer,
    {
        let value: u64 = (*value).into();
        if value > MAX_SAFE {
            return Err(ser::Error::custom(too_large(value)));
        }
        serializer.serialize_u64(value)
    }

    /// Reads a `u64`; an error when it is above [`MAX_SAFE`].
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u64, D::Error> {
        let value = u64::deserialize(deserializer)?;
        if value > MAX_SAFE {
            return Err(de::Error::custom(too_large(value)));
        }
        Ok(value)
    }
}

/// `#[serde(with)]` for an `Option<u64>` field whose value must stay at most [`MAX_SAFE`]. Use it
/// with `skip_serializing_if = "Option::is_none", default`, as every `Option` field.
pub(crate) mod safe_opt {
    use serde::{Deserializer, Serializer};

    /// Writes `Some(value)` as `serialize_some` does, `None` as `serialize_none`. (Generic only
    /// because serde passes the field by reference.)
    pub(crate) fn serialize<V, S>(value: &V, serializer: S) -> Result<S::Ok, S::Error>
    where
        V: Copy + Into<Option<u64>>,
        S: Serializer,
    {
        match (*value).into() {
            Some(value) => serializer.serialize_some(&Checked(value)),
            None => serializer.serialize_none(),
        }
    }

    /// Reads an optional `u64`; an error when it is above [`MAX_SAFE`](crate::MAX_SAFE).
    pub(crate) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<u64>, D::Error> {
        let value: Option<Checked> = serde::Deserialize::deserialize(deserializer)?;
        Ok(value.map(|checked| checked.0))
    }

    /// A `u64` that goes through [`super::safe`].
    struct Checked(u64);

    impl serde::Serialize for Checked {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            super::safe::serialize(&self.0, serializer)
        }
    }

    impl<'de> serde::Deserialize<'de> for Checked {
        fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
            super::safe::deserialize(deserializer).map(Checked)
        }
    }
}
