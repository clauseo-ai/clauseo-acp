//! One setting, typed. Every source (`settings.json`, `policy.json`, the registry, managed
//! preferences) turns its keys into dotted keys with JSON values, and this module types them
//! the same way for all of them.
//!
//! # Adding a key
//!
//! A new key goes in every one of these places, or some source never reads it:
//! - its field in `schema.rs` (`Settings` and the section's struct), which moves the schema
//!   snapshot;
//! - its row in [`KEYS`], with its kind: the macOS shim asks managed preferences only for these
//!   keys, and the registry calls any other value name unknown;
//! - a [`Leaf`] variant, and its arms in `Leaf::parse`, `Leaf::key` and `Leaf::apply`, and
//!   in `reset`, which a value that does not type puts the key back at its default
//!   (`leaf/tests.rs`' `a_refused_key_goes_back_to_its_default` fails for a row with no arm);
//! - for a `ui` key, its field in `UiPatch` (`ui.rs`, and its name in `UiPatch::keys`) and in
//!   `Settings::apply_ui_patch`. Two tests in `ui/tests.rs` fail when `keys()` misses a `ui`
//!   key or differs from the names the writer saves: `the_keys_are_the_names_the_writer_saves`
//!   and `the_keys_a_patch_sets_are_the_ui_policy_keys_in_field_order`;
//! - a sample value in `leaf/tests.rs`' `every_fixed_key_types_and_the_kinds_agree`, and a
//!   value that is not the default in `a_refused_key_goes_back_to_its_default`.
//!
//! `schema::tests::the_keys_and_their_kinds_are_the_schemas` fails when the schema and [`KEYS`]
//! disagree, and the two samples tests fail when [`KEYS`] has a row without a sample.

#[cfg(test)]
mod tests;

use crate::policy::{Kind, kind_of};
use crate::schema::{AgentLaunch, Density, FontSize, Language, McpServerLaunch, Settings, Theme};
use crate::warning::{Origin, Warning, WarningKind};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Every key with a fixed name, and its kind. An agent's key is `agents.<id>`, an object.
/// `schema::tests::the_keys_and_their_kinds_are_the_schemas` holds this to the schema.
pub(crate) const KEYS: [(&str, Kind); 6] = [
    ("matters.default_location", Kind::String),
    ("mcp.servers", Kind::Object),
    ("ui.theme", Kind::String),
    ("ui.font_size", Kind::Number),
    ("ui.density", Kind::String),
    ("ui.language", Kind::String),
];

/// The keys that decide what an agent can reach. A policy source that cannot be read locks each
/// at its default (ADR 0009): its value there is unknown, and the user's must not stand in.
pub(crate) const SECURITY_KEYS: [&str; 1] = ["mcp.servers"];

/// The fields an [`AgentLaunch`] reads; any other is an unknown key.
const AGENT_FIELDS: [&str; 3] = ["command", "args", "env"];

/// The fields an [`McpServerLaunch`] reads; any other is an unknown key.
const MCP_SERVER_FIELDS: [&str; 4] = ["command", "args", "env", "enabled"];

/// The longest MCP server name.
const MCP_NAME_MAX: usize = 64;

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
    /// The whole `mcp.servers` list: a policy value replaces the user's list, never merges.
    McpServers(BTreeMap<String, McpServerLaunch>),
    Theme(Theme),
    FontSize(FontSize),
    Density(Density),
    Language(Language),
    /// A known key whose value does not type, or that a policy source refused. Applied, it puts
    /// the key back at its default; for `agents.<id>`, the entry for that id is dropped. A
    /// policy also locks the key (fail closed, Rohan, 2026-10-31).
    Refused(String),
}

/// The agent id in an `agents.<id>` key.
pub(crate) fn agent_id(key: &str) -> Option<&str> {
    key.strip_prefix("agents.").filter(|id| !id.is_empty())
}

/// Types every raw value of a source, in order, with one rule for every source. A value that
/// cannot be typed gets a warning, and when its key is known it becomes [`Leaf::Refused`]: the
/// key goes back to its default, so a broken value never leaves an earlier one in force. A key
/// the source sets twice keeps its last value, valid or not, as `policy.json` always did; the
/// result holds each key once. `refused` holds the known keys a policy source's own reader
/// already refused (a registry value of the wrong type); they count as after every raw value,
/// so a refused key wins wherever its raw values sit.
pub(crate) fn typed(
    raws: Vec<Raw>,
    refused: Vec<String>,
    warnings: &mut Vec<Warning>,
) -> Vec<Leaf> {
    let mut leaves = Vec::with_capacity(raws.len());
    for raw in raws {
        let mut kinds = Vec::new();
        match Leaf::parse(&raw.key, raw.value, &mut kinds) {
            Ok(leaf) => leaves.push(leaf),
            Err(kind) => {
                if kind_of(&raw.key).is_some() {
                    leaves.push(Leaf::Refused(raw.key.clone()));
                }
                kinds.push(kind);
            }
        }
        warnings.extend(kinds.into_iter().map(|kind| Warning {
            origin: raw.origin.clone(),
            kind,
        }));
    }
    leaves.extend(refused.into_iter().map(Leaf::Refused));
    keep_last(leaves, Leaf::key)
}

/// [`typed`] for a policy source. A security-relevant key the source names in a form that does
/// not set it is also refused, so it locks at its default (ADR 0009): a section that is not an
/// object (`"mcp": "off"`), or a section or key that differs from it only in case (`"MCP"`,
/// `mcp.Servers`). What the firm meant there is unknown, and the user's value must not stand in.
pub(crate) fn typed_policy(
    raws: Vec<Raw>,
    mut refused: Vec<String>,
    warnings: &mut Vec<Warning>,
) -> Vec<Leaf> {
    refused.extend(security_keys_misnamed(&raws, warnings));
    typed(raws, refused, warnings)
}

/// The security-relevant keys that `raws` or `warnings` name in a form that does not set them.
fn security_keys_misnamed(raws: &[Raw], warnings: &[Warning]) -> Vec<String> {
    SECURITY_KEYS
        .iter()
        .filter(|&&key| {
            let section = key.split('.').next().unwrap_or(key);
            let near = |name: &str| {
                name != key
                    && (name.eq_ignore_ascii_case(key) || name.eq_ignore_ascii_case(section))
            };
            raws.iter().any(|raw| near(&raw.key))
                || warnings.iter().any(|warning| match &warning.kind {
                    WarningKind::UnknownKey { key } | WarningKind::InvalidValue { key, .. } => {
                        near(key)
                    }
                    WarningKind::Unreadable { .. }
                    | WarningKind::Syntax { .. }
                    | WarningKind::RelativeAgentCommand { .. } => false,
                })
        })
        .map(|&key| key.to_owned())
        .collect()
}

/// The last item of each key, in the order those last items came. A source that sets a key
/// twice keeps the last one: a section of `settings.json` as a whole ([`crate::json::read`]),
/// and each key's value ([`typed`]).
pub(crate) fn keep_last<T>(items: Vec<T>, key: impl Fn(&T) -> String) -> Vec<T> {
    let mut seen = BTreeSet::new();
    let mut last: Vec<T> = items
        .into_iter()
        .rev()
        .filter(|item| seen.insert(key(item)))
        .collect();
    last.reverse();
    last
}

impl Leaf {
    /// Types one value. `unknown` gets a warning for each field of an agent or MCP server this
    /// version does not know (the entry itself still loads), and for each MCP server entry left
    /// out (the rest of the list still loads).
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
            "mcp.servers" => mcp_servers(key, value, unknown).map(Leaf::McpServers),
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
            Leaf::McpServers(_) => "mcp.servers".to_owned(),
            Leaf::Theme(_) => "ui.theme".to_owned(),
            Leaf::FontSize(_) => "ui.font_size".to_owned(),
            Leaf::Density(_) => "ui.density".to_owned(),
            Leaf::Language(_) => "ui.language".to_owned(),
            Leaf::Refused(key) => key.clone(),
        }
    }

    /// Whether this is a value, not a [`Leaf::Refused`] key.
    pub(crate) fn is_value(&self) -> bool {
        !matches!(self, Leaf::Refused(_))
    }

    /// Writes the value into `settings`, replacing what was there. An agent replaces the whole
    /// entry for its id.
    pub(crate) fn apply(self, settings: &mut Settings) {
        match self {
            Leaf::DefaultLocation(path) => settings.matters.default_location = path,
            Leaf::Agent(id, launch) => {
                settings.agents.insert(id, launch);
            }
            Leaf::McpServers(servers) => settings.mcp.servers = servers,
            Leaf::Theme(theme) => settings.ui.theme = theme,
            Leaf::FontSize(size) => settings.ui.font_size = size,
            Leaf::Density(density) => settings.ui.density = density,
            Leaf::Language(language) => settings.ui.language = language,
            Leaf::Refused(key) => reset(&key, settings),
        }
    }
}

/// Puts the setting `key` back to its default; for `agents.<id>`, drops that agent.
fn reset(key: &str, settings: &mut Settings) {
    match key {
        "matters.default_location" => settings.matters.default_location = None,
        "mcp.servers" => settings.mcp.servers.clear(),
        "ui.theme" => settings.ui.theme = Theme::default(),
        "ui.font_size" => settings.ui.font_size = FontSize::default(),
        "ui.density" => settings.ui.density = Density::default(),
        "ui.language" => settings.ui.language = Language::default(),
        _ => {
            if let Some(id) = agent_id(key) {
                settings.agents.remove(id);
            }
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

/// The `mcp.servers` list. A value that is not an object refuses the key (a policy's locks the
/// list empty). Inside it, an entry with a bad name, a value that does not type or a relative
/// `command` is left out with a warning in `dropped`, and the other entries load.
fn mcp_servers(
    key: &str,
    value: Value,
    dropped: &mut Vec<WarningKind>,
) -> Result<BTreeMap<String, McpServerLaunch>, WarningKind> {
    let Value::Object(entries) = value else {
        return Err(WarningKind::InvalidValue {
            key: key.to_owned(),
            message: "it must be an object from server name to server".to_owned(),
        });
    };
    let mut servers = BTreeMap::new();
    for (name, entry) in entries {
        let entry_key = format!("{key}.{name}");
        if !mcp_name_ok(&name) {
            dropped.push(WarningKind::InvalidValue {
                key: entry_key,
                message: format!("a server name is 1 to {MCP_NAME_MAX} letters, digits, _ or -"),
            });
            continue;
        }
        if let Value::Object(fields) = &entry {
            dropped.extend(
                fields
                    .keys()
                    .filter(|field| !MCP_SERVER_FIELDS.contains(&field.as_str()))
                    .map(|field| WarningKind::UnknownKey {
                        key: format!("{entry_key}.{field}"),
                    }),
            );
        }
        match typed_value::<McpServerLaunch>(&entry_key, entry) {
            Ok(server) if Path::new(&server.command).is_absolute() => {
                servers.insert(name, server);
            }
            Ok(server) => dropped.push(WarningKind::InvalidValue {
                key: entry_key,
                message: format!("{} is not an absolute path", server.command),
            }),
            Err(kind) => dropped.push(kind),
        }
    }
    Ok(servers)
}

/// Whether `name` is 1 to [`MCP_NAME_MAX`] ASCII letters, digits, `_` or `-`.
fn mcp_name_ok(name: &str) -> bool {
    (1..=MCP_NAME_MAX).contains(&name.len())
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

/// Reads a JSON value as `T`, naming the key when it cannot.
fn typed_value<T: DeserializeOwned>(key: &str, value: Value) -> Result<T, WarningKind> {
    serde_json::from_value(value).map_err(|error| WarningKind::InvalidValue {
        key: key.to_owned(),
        message: error.to_string(),
    })
}
