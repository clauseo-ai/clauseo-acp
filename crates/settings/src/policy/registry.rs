//! The Windows registry: one value per dotted key under `HKLM\SOFTWARE\Policies\Clauseo\Clauseo`,
//! as Intune's Settings Catalog and ADMX push them.
//!
//! `REG_DWORD` is a number, or a boolean (0 or 1) where the setting is one. `REG_SZ` is a string,
//! or JSON text where the setting is an object (`agents.<id>`, `plans.<id>`, `mcp.servers`).
//! `REG_MULTI_SZ` is a list of strings. Value names are compared without case, as Windows does
//! (`UI.Theme` is `ui.theme`); an agent or plan id keeps the case it is written in. A known key's
//! value of another type gets a warning and locks the key at its default; an unknown value name is
//! only a warning.

#[cfg(test)]
mod tests;

use super::{Kind, PolicyValues, kind_of};
use crate::leaf::{self, Places, Raw, SECURITY_KEYS};
use crate::lock_reason::{LockCause, LockReason, SourceKind, shown_section};
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
    let (raws, refused, warnings, places, duplicates) = raw_values(values, kinds);
    leaf::typed_policy(raws, &[], refused, warnings, &places).with_more_reasons(duplicates)
}

/// Each value as JSON, by its setting's kind; a warning for each that cannot be, and the known
/// keys among those (refused, so locked at their default); each one's place among the values,
/// from 1; and why each section a JSON value with a duplicate key locks is locked.
///
/// A `REG_SZ` JSON value (`mcp.servers`, `plans.<id>`) that holds a key twice in one object
/// is refused, as a `policy.json` that does is: which copy the firm meant is unknown. It locks
/// its section's security keys (`mcp.servers` empty, no plan at all), or only itself in a
/// section with none, with a [`LockCause::DuplicateKey`] reason; the other keys stand.
pub(crate) fn raw_values(
    values: Vec<(String, RegValue)>,
    kinds: fn(&str) -> Option<Kind>,
) -> RawValues {
    let origin = Origin::Store {
        name: SOURCE.to_owned(),
    };
    let mut raws = Vec::with_capacity(values.len());
    let mut refused = Vec::new();
    let mut warnings = Vec::new();
    let mut plans_seen = 0;
    let mut places = Places::default();
    let mut duplicates = Vec::new();
    for (at, (name, value)) in values.into_iter().enumerate() {
        let key = canonical(name);
        plans_seen += usize::from(key.starts_with("plans."));
        if kinds(&key) == Some(Kind::Object)
            && let RegValue::Sz(text) = &value
            && matches!(crate::json::duplicate(text.as_bytes()), Ok(Some(_)))
        {
            let reason = duplicate_reason(&key, at + 1);
            refused.extend(reason.keys.iter().cloned());
            duplicates.push(reason);
            places.warnings.push(at + 1);
            warnings.push(Warning {
                origin: origin.clone(),
                kind: WarningKind::InvalidValue {
                    key: crate::plans::shown_plan_key(&key, plans_seen),
                    message: DUPLICATE.to_owned(),
                },
            });
            continue;
        }
        let converted = match kinds(&key) {
            Some(kind) => json_value(kind, value),
            None => Err(None),
        };
        let place = at + 1;
        match converted {
            Ok(value) => {
                places.raws.push(place);
                raws.push(Raw {
                    key,
                    value,
                    origin: origin.clone(),
                });
            }
            Err(message) => {
                places.warnings.push(place);
                warnings.push(Warning {
                    origin: origin.clone(),
                    kind: match message {
                        Some(message) => {
                            refused.push(key.clone());
                            WarningKind::InvalidValue { key, message }
                        }
                        // A `plans.` name that is no plan's can be pasted text: shown by place.
                        None => WarningKind::UnknownKey {
                            key: crate::plans::shown_plan_key(&key, plans_seen),
                        },
                    },
                });
            }
        }
    }
    (raws, refused, warnings, places, duplicates)
}

/// What [`raw_values`] returns: the values as JSON, the refused keys, the warnings, each one's
/// place, and the reasons for the sections a duplicate key locks.
pub(crate) type RawValues = (Vec<Raw>, Vec<String>, Vec<Warning>, Places, Vec<LockReason>);

/// The warning for a `REG_SZ` JSON value that holds a key twice.
const DUPLICATE: &str = "the REG_SZ holds a key twice in one object, so which copy is meant is \
                         unknown; it is locked off";

/// Why the section of `key`, a JSON value with a duplicate key at the `place`th value, is
/// locked: its security keys (`plans.` for a plan), or `key` alone when it has none.
fn duplicate_reason(key: &str, place: usize) -> LockReason {
    let section = key.split('.').next().unwrap_or(key);
    let mut keys: Vec<String> = SECURITY_KEYS
        .iter()
        .filter(|known| known.split('.').next() == Some(section))
        .map(|&known| known.to_owned())
        .collect();
    if keys.is_empty() {
        keys.push(key.to_owned());
    }
    LockReason {
        keys,
        cause: LockCause::DuplicateKey,
        source: SourceKind::Registry,
        section: shown_section(section),
        place: u32::try_from(place).unwrap_or(u32::MAX),
    }
}

/// The dotted key a value name stands for. Windows compares value names without case, so a name
/// that matches a key but for case is that key. `agents.` and `plans.` match without case too,
/// but the id after either keeps its case, since ids are case-sensitive. Any other name is kept
/// as written.
fn canonical(name: String) -> String {
    if let Some(&(key, _)) = leaf::KEYS
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(&name))
    {
        return key.to_owned();
    }
    for section in BY_ID {
        if let (Some(prefix), Some(id)) = (name.get(..section.len()), name.get(section.len()..))
            && prefix.eq_ignore_ascii_case(section)
        {
            return format!("{section}{id}");
        }
    }
    name
}

/// The prefixes of the keys whose entries go by id: an agent's and a plan's.
const BY_ID: [&str; 2] = ["agents.", "plans."];

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
