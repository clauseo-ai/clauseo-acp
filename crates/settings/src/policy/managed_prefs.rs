//! macOS managed preferences for `ai.clauseo.desktop`, as an MDM configuration profile pushes
//! them.
//!
//! A value counts as policy only when it is forced (`CFPreferencesAppValueIsForced`): the same
//! call also returns what a user wrote with `defaults write`, which is not policy. Keys are the
//! dotted keys, except agents: the key `agents` holds a dictionary from agent id to launch spec,
//! since managed preferences are read key by key and cannot list the ids. An `agents.<id>` key
//! is never read there (the shim never asks for one), and [`parse`] calls it unknown.

#[cfg(test)]
mod tests;

use super::{PolicyRead, PolicyValues};
use crate::leaf::{self, Raw};
use crate::warning::{Origin, Warning, WarningKind};
use serde_json::{Map, Number, Value};

/// The name the managed preferences source goes by in warnings and the policy status.
pub const SOURCE: &str = "managed preferences";

/// The key whose dictionary holds every agent.
pub const AGENTS_KEY: &str = "agents";

/// A property list value, as Core Foundation returns it.
#[derive(Debug, Clone, PartialEq)]
pub enum PlistValue {
    /// A boolean.
    Bool(bool),
    /// An integer.
    Integer(i64),
    /// A floating-point number.
    Real(f64),
    /// A string.
    String(String),
    /// An array.
    Array(Vec<PlistValue>),
    /// A dictionary, in Core Foundation's order.
    Dict(Vec<(String, PlistValue)>),
    /// A date or data, which no setting takes, by its type's name.
    Other(&'static str),
}

/// The forced values, typed. `Absent` when none of them is forced.
pub fn parse(values: Vec<(String, PlistValue, bool)>) -> PolicyRead {
    let forced: Vec<_> = values
        .into_iter()
        .filter_map(|(key, value, forced)| forced.then_some((key, value)))
        .collect();
    if forced.is_empty() {
        return PolicyRead::Absent;
    }
    let mut entries = Vec::new();
    for (key, value) in forced {
        match value {
            PlistValue::Dict(agents) if key == AGENTS_KEY => entries.extend(
                agents
                    .into_iter()
                    .map(|(id, launch)| (format!("{AGENTS_KEY}.{id}"), Some(launch))),
            ),
            value if leaf::KEYS.iter().any(|&(known, _)| known == key) => {
                entries.push((key, Some(value)));
            }
            _ => entries.push((key, None)),
        }
    }
    let origin = Origin::Store {
        name: SOURCE.to_owned(),
    };
    let mut raws = Vec::new();
    let mut refused = Vec::new();
    let mut warnings = Vec::new();
    for (key, value) in entries {
        let kind = match value.map(plain_json) {
            Some(Ok(value)) => {
                raws.push(Raw {
                    key,
                    value,
                    origin: origin.clone(),
                });
                continue;
            }
            Some(Err(message)) => {
                refused.push(key.clone());
                WarningKind::InvalidValue { key, message }
            }
            None if key == AGENTS_KEY => WarningKind::InvalidValue {
                key,
                message: "it must be a dictionary from agent id to launch spec".to_owned(),
            },
            None => WarningKind::UnknownKey { key },
        };
        warnings.push(Warning {
            origin: origin.clone(),
            kind,
        });
    }
    let leaves = leaf::typed(raws, refused, &mut warnings);
    PolicyRead::Read(PolicyValues::new(leaves, warnings))
}

/// The JSON value a property list value stands for, or why it stands for none.
fn plain_json(value: PlistValue) -> Result<Value, String> {
    Ok(match value {
        PlistValue::Bool(flag) => Value::Bool(flag),
        PlistValue::Integer(number) => Value::from(number),
        PlistValue::Real(number) => Value::Number(
            Number::from_f64(number).ok_or_else(|| format!("{number} is not a finite number"))?,
        ),
        PlistValue::String(text) => Value::String(text),
        PlistValue::Array(items) => Value::Array(
            items
                .into_iter()
                .map(plain_json)
                .collect::<Result<_, _>>()?,
        ),
        PlistValue::Dict(entries) => Value::Object(
            entries
                .into_iter()
                .map(|(key, value)| Ok((key, plain_json(value)?)))
                .collect::<Result<Map<_, _>, String>>()?,
        ),
        PlistValue::Other(type_name) => return Err(format!("no setting takes a {type_name}")),
    })
}
