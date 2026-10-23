//! The Windows registry: one value per dotted key under `HKLM\SOFTWARE\Policies\Clauseo\Clauseo`,
//! as Intune's Settings Catalog and ADMX push them.
//!
//! `REG_DWORD` is a number, or a boolean (0 or 1) where the setting is one. `REG_SZ` is a string,
//! or JSON text where the setting is an object (`agents.<id>`). `REG_MULTI_SZ` is a list of
//! strings. Any other type is left out with a warning.

#[cfg(test)]
mod tests;

use super::{Kind, PolicyValues, kind_of};
use crate::leaf::{self, Raw};
use crate::warning::{Origin, Warning, WarningKind};
use serde_json::Value;

/// The name the registry source goes by in warnings and the policy status.
pub const SOURCE: &str = "registry";

/// The key policy values sit under, in `HKEY_LOCAL_MACHINE`, read through the 64-bit view.
pub const KEY: &str = r"SOFTWARE\Policies\Clauseo\Clauseo";

/// One registry value, as the OS returns it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegValue {
    /// `REG_DWORD`.
    Dword(u32),
    /// `REG_SZ`.
    Sz(String),
    /// `REG_MULTI_SZ`.
    MultiSz(Vec<String>),
    /// Any other type, by its number (`REG_BINARY` is 3).
    Other(u32),
}

/// The values the key holds, typed by each setting's kind.
pub fn parse(values: Vec<(String, RegValue)>) -> PolicyValues {
    parse_with(values, kind_of)
}

/// [`parse`], with the kinds from `kinds`, so tests can read kinds no M1 setting has.
pub(crate) fn parse_with(
    values: Vec<(String, RegValue)>,
    kinds: fn(&str) -> Option<Kind>,
) -> PolicyValues {
    let (raws, mut warnings) = raw_values(values, kinds);
    let leaves = leaf::typed(raws, &mut warnings);
    PolicyValues::new(leaves, warnings)
}

/// Each value as JSON, by its setting's kind; a warning for each that cannot be.
pub(crate) fn raw_values(
    values: Vec<(String, RegValue)>,
    kinds: fn(&str) -> Option<Kind>,
) -> (Vec<Raw>, Vec<Warning>) {
    let origin = Origin::Store {
        name: SOURCE.to_owned(),
    };
    let mut raws = Vec::with_capacity(values.len());
    let mut warnings = Vec::new();
    for (key, value) in values {
        let converted = match kinds(&key) {
            Some(kind) => json_value(kind, value),
            None => Err(None),
        };
        match converted {
            Ok(value) => raws.push(Raw {
                key,
                value,
                origin: origin.clone(),
            }),
            Err(message) => warnings.push(Warning {
                origin: origin.clone(),
                kind: match message {
                    Some(message) => WarningKind::InvalidValue { key, message },
                    None => WarningKind::UnknownKey { key },
                },
            }),
        }
    }
    (raws, warnings)
}

/// The JSON value a registry value stands for, or why it stands for none.
fn json_value(kind: Kind, value: RegValue) -> Result<Value, Option<String>> {
    match (kind, value) {
        (Kind::Boolean, RegValue::Dword(0)) => Ok(Value::Bool(false)),
        (Kind::Boolean, RegValue::Dword(1)) => Ok(Value::Bool(true)),
        (Kind::Boolean, RegValue::Dword(other)) => Err(Some(format!(
            "a boolean is the REG_DWORD 0 or 1, not {other}"
        ))),
        (Kind::Number, RegValue::Dword(number)) => Ok(Value::from(number)),
        (Kind::String, RegValue::Sz(text)) => Ok(Value::String(text)),
        (Kind::Object, RegValue::Sz(text)) => serde_json::from_str(&text)
            .map_err(|error| Some(format!("the REG_SZ is not JSON: {error}"))),
        (Kind::StringArray, RegValue::MultiSz(items)) => {
            Ok(Value::Array(items.into_iter().map(Value::String).collect()))
        }
        (kind, value) => Err(Some(format!(
            "it takes {}, not {}",
            expected(kind),
            type_name(&value)
        ))),
    }
}

/// The registry type a kind takes.
fn expected(kind: Kind) -> &'static str {
    match kind {
        Kind::Boolean | Kind::Number => "a REG_DWORD",
        Kind::String => "a REG_SZ",
        Kind::Object => "a REG_SZ holding JSON",
        Kind::StringArray => "a REG_MULTI_SZ",
    }
}

/// A registry value's type, as Windows names it.
fn type_name(value: &RegValue) -> String {
    match value {
        RegValue::Dword(_) => "a REG_DWORD".to_owned(),
        RegValue::Sz(_) => "a REG_SZ".to_owned(),
        RegValue::MultiSz(_) => "a REG_MULTI_SZ".to_owned(),
        RegValue::Other(number) => format!("registry type {number}"),
    }
}
