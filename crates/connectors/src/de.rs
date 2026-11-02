//! Lenient field readers for the connector's JSON. A field of the wrong type reads as its default
//! rather than failing the whole answer: the connector's shapes are not final, and a parser that
//! fails on a new number format would hide the slot.

use std::collections::BTreeMap;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;

fn value<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Value, D::Error> {
    Value::deserialize(deserializer)
}

/// A string as is, a number as its JSON text; anything else is empty.
fn text_of(value: Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

/// A whole number from a JSON integer or a string of one.
fn int_of(value: &Value) -> Option<i64> {
    match value {
        Value::Number(number) => number
            .as_i64()
            .or_else(|| number.as_u64().map(|_| i64::MAX)),
        Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// A rupee amount or any short text: a string as written, a number as its JSON text (`0.1` for
/// `0.10`: `serde_json` keeps no more of a number's text), anything else empty.
pub(crate) fn text<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    Ok(text_of(value(deserializer)?).unwrap_or_default())
}

/// [`text`], with `null` and other types as `None`.
pub(crate) fn opt_text<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<Option<String>, D::Error> {
    Ok(text_of(value(deserializer)?))
}

/// A whole number, from an integer or a string of one; anything else is 0.
pub(crate) fn int<'de, D: Deserializer<'de>>(deserializer: D) -> Result<i64, D::Error> {
    Ok(int_of(&value(deserializer)?).unwrap_or_default())
}

/// [`int`], with `null` and other types as `None`.
pub(crate) fn opt_int<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<i64>, D::Error> {
    Ok(int_of(&value(deserializer)?))
}

/// `true` only for JSON `true`.
pub(crate) fn flag<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    Ok(value(deserializer)?.as_bool().unwrap_or_default())
}

/// A map of names to whole numbers; entries of another type are left out.
pub(crate) fn prices<'de, D: Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, i64>, D::Error> {
    let Value::Object(map) = value(deserializer)? else {
        return Ok(BTreeMap::new());
    };
    Ok(map
        .into_iter()
        .filter_map(|(name, price)| int_of(&price).map(|price| (name, price)))
        .collect())
}

/// A list; items that do not parse are left out, and a non-list is empty.
pub(crate) fn list<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    let Value::Array(items) = value(deserializer)? else {
        return Ok(Vec::new());
    };
    Ok(items
        .into_iter()
        .filter_map(|item| serde_json::from_value(item).ok())
        .collect())
}

/// An object that parses as `T`, or `None`.
pub(crate) fn opt_object<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    let value = value(deserializer)?;
    if !value.is_object() {
        return Ok(None);
    }
    Ok(serde_json::from_value(value).ok())
}
