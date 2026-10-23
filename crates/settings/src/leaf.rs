//! One setting, typed. Every source (`settings.json`, `policy.json`, the registry, managed
//! preferences) turns its keys into dotted keys with JSON values, and this module types them
//! the same way for all of them.

#[cfg(test)]
mod tests;

use crate::policy::Kind;
use crate::schema::{AgentLaunch, Density, FontSize, Language, Settings, Theme};
use crate::warning::{Origin, Warning, WarningKind};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Every key with a fixed name, and its kind. An agent's key is `agents.<id>`, an object.
pub(crate) const KEYS: [(&str, Kind); 5] = [
    ("matters.default_location", Kind::String),
    ("ui.theme", Kind::String),
    ("ui.font_size", Kind::Number),
    ("ui.density", Kind::String),
    ("ui.language", Kind::String),
];

/// The fields an [`AgentLaunch`] reads; any other is an unknown key.
const AGENT_FIELDS: [&str; 3] = ["command", "args", "env"];

/// A key and its value as a source holds it, before typing.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Raw {
    pub key: String,
    pub value: Value,
    pub origin: Origin,
}

/// One typed setting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Leaf {
    DefaultLocation(Option<PathBuf>),
    Agent(String, AgentLaunch),
    Theme(Theme),
    FontSize(FontSize),
    Density(Density),
    Language(Language),
}

/// The agent id in an `agents.<id>` key.
pub(crate) fn agent_id(key: &str) -> Option<&str> {
    key.strip_prefix("agents.").filter(|id| !id.is_empty())
}

/// Types every raw value, in order. A value that cannot be typed is left out with a warning.
pub(crate) fn typed(raws: Vec<Raw>, warnings: &mut Vec<Warning>) -> Vec<Leaf> {
    let mut leaves = Vec::with_capacity(raws.len());
    for raw in raws {
        let mut kinds = Vec::new();
        match Leaf::parse(&raw.key, raw.value, &mut kinds) {
            Ok(leaf) => leaves.push(leaf),
            Err(kind) => kinds.push(kind),
        }
        warnings.extend(kinds.into_iter().map(|kind| Warning {
            origin: raw.origin.clone(),
            kind,
        }));
    }
    leaves
}

impl Leaf {
    /// Types one value. `unknown` gets a warning for each field of an agent this version does
    /// not know; the agent itself still loads.
    fn parse(key: &str, value: Value, unknown: &mut Vec<WarningKind>) -> Result<Leaf, WarningKind> {
        match key {
            "matters.default_location" => {
                let path: Option<PathBuf> = typed_value(key, value)?;
                match path {
                    Some(path) if !path.is_absolute() => Err(WarningKind::InvalidValue {
                        key: key.to_owned(),
                        message: format!("{} is not an absolute path", path.display()),
                    }),
                    path => Ok(Leaf::DefaultLocation(path)),
                }
            }
            "ui.theme" => typed_value(key, value).map(Leaf::Theme),
            "ui.font_size" => typed_value(key, value).map(Leaf::FontSize),
            "ui.density" => typed_value(key, value).map(Leaf::Density),
            "ui.language" => typed_value(key, value).map(Leaf::Language),
            _ => match agent_id(key) {
                Some(id) => agent(key, id, value, unknown),
                None => Err(WarningKind::UnknownKey {
                    key: key.to_owned(),
                }),
            },
        }
    }

    /// The dotted key a policy locks when it sets this value.
    pub(crate) fn key(&self) -> String {
        match self {
            Leaf::DefaultLocation(_) => "matters.default_location".to_owned(),
            Leaf::Agent(id, _) => format!("agents.{id}"),
            Leaf::Theme(_) => "ui.theme".to_owned(),
            Leaf::FontSize(_) => "ui.font_size".to_owned(),
            Leaf::Density(_) => "ui.density".to_owned(),
            Leaf::Language(_) => "ui.language".to_owned(),
        }
    }

    /// Writes the value into `settings`, replacing what was there. An agent replaces the whole
    /// entry for its id.
    pub(crate) fn apply(self, settings: &mut Settings) {
        match self {
            Leaf::DefaultLocation(path) => settings.matters.default_location = path,
            Leaf::Agent(id, launch) => {
                settings.agents.insert(id, launch);
            }
            Leaf::Theme(theme) => settings.ui.theme = theme,
            Leaf::FontSize(size) => settings.ui.font_size = size,
            Leaf::Density(density) => settings.ui.density = density,
            Leaf::Language(language) => settings.ui.language = language,
        }
    }
}

/// An agent's launch spec. Its command must be an absolute path: `process` refuses others.
fn agent(
    key: &str,
    id: &str,
    value: Value,
    unknown: &mut Vec<WarningKind>,
) -> Result<Leaf, WarningKind> {
    if let Value::Object(fields) = &value {
        unknown.extend(
            fields
                .keys()
                .filter(|field| !AGENT_FIELDS.contains(&field.as_str()))
                .map(|field| WarningKind::UnknownKey {
                    key: format!("{key}.{field}"),
                }),
        );
    }
    let launch: AgentLaunch = typed_value(key, value)?;
    if Path::new(&launch.command).is_absolute() {
        Ok(Leaf::Agent(id.to_owned(), launch))
    } else {
        Err(WarningKind::RelativeAgentCommand {
            id: id.to_owned(),
            command: launch.command,
        })
    }
}

/// Reads a JSON value as `T`, naming the key when it cannot.
fn typed_value<T: DeserializeOwned>(key: &str, value: Value) -> Result<T, WarningKind> {
    serde_json::from_value(value).map_err(|error| WarningKind::InvalidValue {
        key: key.to_owned(),
        message: error.to_string(),
    })
}
