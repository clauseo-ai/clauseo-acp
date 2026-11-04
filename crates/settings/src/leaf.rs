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
//!   value that is not the default in `a_refused_key_goes_back_to_its_default`;
//! - for a key that decides what an agent can reach, or what Clauseo sends off the machine, its
//!   name in [`SECURITY_KEYS`] and its arm in [`refuse`], which sets the value it is locked at
//!   when its policy is unknown;
//! - for a key a policy may only turn off, its name in [`OFF_ONLY_KEYS`] (`Effective::load`
//!   ignores a policy `true` for it, with a warning).
//!
//! `schema::tests::the_keys_and_their_kinds_are_the_schemas` fails when the schema and [`KEYS`]
//! disagree, and the two samples tests fail when [`KEYS`] has a row without a sample.

#[cfg(test)]
mod tests;

use crate::policy::{Kind, kind_of};
use crate::schema::{
    AgentLaunch, Density, FontSize, Language, Lite, McpServerLaunch, Motion, PrivacySettings,
    Settings, Theme, ToolsSettings,
};
use crate::warning::{Origin, Warning, WarningKind};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Every key with a fixed name, and its kind. An agent's key is `agents.<id>`, an object.
/// `schema::tests::the_keys_and_their_kinds_are_the_schemas` holds this to the schema.
pub(crate) const KEYS: [(&str, Kind); 12] = [
    ("matters.default_location", Kind::String),
    ("mcp.servers", Kind::Object),
    ("ui.theme", Kind::String),
    ("ui.font_size", Kind::Number),
    ("ui.density", Kind::String),
    ("ui.language", Kind::String),
    ("ui.lite", Kind::String),
    ("ui.motion", Kind::String),
    ("tools.web_search", Kind::Boolean),
    ("tools.browser", Kind::Boolean),
    ("privacy.counts", Kind::Boolean),
    ("privacy.crash_reports", Kind::Boolean),
];

/// The keys that decide what an agent can reach, or what Clauseo sends off the machine. A policy
/// source that cannot be read locks each off with [`refuse`] (ADR 0009, R21; the `privacy` keys,
/// decision record 0036): its value there is unknown, and the user's must not stand in. Off is
/// not always the default: both `tools` keys and `privacy.counts` default to on, and locking
/// them at their default would fail open.
pub(crate) const SECURITY_KEYS: [&str; 5] = [
    "mcp.servers",
    "tools.web_search",
    "tools.browser",
    "privacy.counts",
    "privacy.crash_reports",
];

/// The keys a policy may only turn off (decision record 0036): a policy `true` for one is
/// ignored with a warning and does not lock it, so the lawyer's own switch stands. A policy
/// `false` locks it, as any policy value does.
pub(crate) const OFF_ONLY_KEYS: [&str; 2] = ["privacy.counts", "privacy.crash_reports"];

/// The MCP server name Clauseo keeps for its own browser tool. An `mcp.servers` entry of this
/// name, in any case, in `settings.json` or a policy, is left out with a warning; the host adds its
/// own entry under it.
pub const RESERVED_MCP_SERVER: &str = "browser";

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
    Lite(Lite),
    Motion(Motion),
    WebSearch(bool),
    Browser(bool),
    Counts(bool),
    CrashReports(bool),
    /// A known key whose value does not type, or that a policy source refused. Applied, it puts
    /// the key back at its default; for `agents.<id>`, the entry for that id is dropped. A
    /// policy also locks the key (fail closed, Rohan, 2026-10-31), and applies it with
    /// [`Leaf::apply_policy`], which locks a security key off rather than at its default.
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

/// [`typed`] for a policy source that lists its own keys (`policy.json` and the registry; macOS
/// managed preferences are read key by key, by exact name, so they use [`typed`]). A
/// security-relevant key is also refused, so locked off (ADR 0009, [`refuse`]), when the source
/// names its topic in a form this version cannot read: its section that is not an object
/// (`"mcp": "off"`), or any unknown key whose name holds the section's name in any case
/// (`"mcp.servers"` at the top of `policy.json`, `"MCP"`, `"mcpServers"`, `mcp.Servers`,
/// `mcp_servers`). What the firm meant there is unknown, so the user's value must not stand in.
/// Each such key gets one warning naming the shape that sets it. An unrelated unknown key, and an
/// entry left out inside a valid list, only warn.
pub(crate) fn typed_policy(
    raws: Vec<Raw>,
    mut refused: Vec<String>,
    warnings: &mut Vec<Warning>,
) -> Vec<Leaf> {
    for (key, origin, found) in security_keys_misnamed(&raws, warnings) {
        warnings.push(Warning {
            origin,
            kind: WarningKind::InvalidValue {
                key: key.to_owned(),
                message: format!(
                    "the policy names {found}, which this version cannot read, so {key} is \
                     locked off; set it as {}",
                    right_shape(key)
                ),
            },
        });
        refused.push(key.to_owned());
    }
    typed(raws, refused, warnings)
}

/// How a policy sets the security-relevant `key`, for the warning of a misnamed one.
fn right_shape(key: &str) -> String {
    let (section, name) = key.split_once('.').unwrap_or((key, ""));
    format!(r#"{{"{section}": {{"{name}": {{...}}}}}} in policy.json, or the registry value {key}"#)
}

/// Each security-relevant key that `raws` or `warnings` name in a form that does not set it,
/// once, with where and how it was first named.
///
/// A name refuses only the keys it could stand for ([`could_stand_for`]): `tools.websearch`
/// refuses `tools.web_search`, not `tools.browser`. A name that could stand for any key of its
/// section (`"tools": "off"`, `tools.brwser`) refuses each, except a sibling the same source
/// sets validly by its exact name, which keeps its value. A source that cannot be read at all
/// still locks every key off (`effective.rs`).
fn security_keys_misnamed(
    raws: &[Raw],
    warnings: &[Warning],
) -> Vec<(&'static str, Origin, String)> {
    let mut found = Vec::new();
    for key in SECURITY_KEYS {
        let section = key.split('.').next().unwrap_or(key);
        // An unknown name about the key's topic, other than an agent's key. This runs before
        // `typed`, so a bad entry inside a valid list (`mcp.servers.<name>`) is not reported
        // yet and stays a warning that drops that entry; a name the firm wrote as
        // `mcp.servers.<name>` itself, at the top level, inside the section or as a registry
        // value, does lock.
        let about = |name: &str| {
            kind_of(name).is_none()
                && !name.starts_with("agents.")
                && name.to_ascii_lowercase().contains(section)
                && could_stand_for(name, key, raws)
        };
        let from_raws = raws
            .iter()
            .find(|raw| about(&raw.key))
            .map(|raw| (raw.origin.clone(), raw.key.clone()));
        let from_warnings = || {
            warnings.iter().find_map(|warning| match &warning.kind {
                WarningKind::UnknownKey { key: name } if about(name) || name == key => {
                    Some((warning.origin.clone(), name.clone()))
                }
                WarningKind::InvalidValue { key: name, .. }
                    if (name == section && !set_validly(raws, key)) || about(name) =>
                {
                    Some((warning.origin.clone(), name.clone()))
                }
                WarningKind::UnknownKey { .. }
                | WarningKind::InvalidValue { .. }
                | WarningKind::Unreadable { .. }
                | WarningKind::Syntax { .. }
                | WarningKind::RelativeAgentCommand { .. } => None,
            })
        };
        if let Some((origin, name)) = from_raws.or_else(from_warnings) {
            found.push((key, origin, format!("`{name}`")));
        }
    }
    found
}

/// Whether the unknown `name`, which holds `key`'s section, could stand for the security key
/// `key`. What the name holds beyond the section, in any case and with no punctuation, is
/// matched against each key of the section: it stands for the keys whose words it holds
/// (`websearch`, `Web-Search`, `web` for `tools.web_search`). One that matches no key of
/// the section, or holds nothing beyond it, could stand for any of them, except a sibling
/// [`set_validly`] by `raws`. A section with one security key (`mcp`) is always it.
fn could_stand_for(name: &str, key: &str, raws: &[Raw]) -> bool {
    let section = key.split('.').next().unwrap_or(key);
    let siblings: Vec<&str> = SECURITY_KEYS
        .into_iter()
        .filter(|other| other.split('.').next() == Some(section))
        .collect();
    if siblings.len() < 2 {
        return true;
    }
    let rest: String = name
        .to_ascii_lowercase()
        .replacen(section, "", 1)
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .collect();
    let holds = |other: &str| {
        !rest.is_empty()
            && other
                .split_once('.')
                .map_or("", |(_, leaf)| leaf)
                .split('_')
                .any(|word| !word.is_empty() && rest.contains(word))
    };
    if siblings.iter().any(|other| holds(other)) {
        holds(key)
    } else {
        !set_validly(raws, key)
    }
}

/// Whether `raws` set `key` by its exact name, the last time to a value that types.
fn set_validly(raws: &[Raw], key: &str) -> bool {
    raws.iter()
        .rev()
        .find(|raw| raw.key == key)
        .is_some_and(|raw| Leaf::parse(key, raw.value.clone(), &mut Vec::new()).is_ok())
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
            "ui.lite" => typed_value(key, value).map(Leaf::Lite),
            "ui.motion" => typed_value(key, value).map(Leaf::Motion),
            "tools.web_search" => typed_value(key, value).map(Leaf::WebSearch),
            "tools.browser" => typed_value(key, value).map(Leaf::Browser),
            "privacy.counts" => typed_value(key, value).map(Leaf::Counts),
            "privacy.crash_reports" => typed_value(key, value).map(Leaf::CrashReports),
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
            Leaf::Lite(_) => "ui.lite".to_owned(),
            Leaf::Motion(_) => "ui.motion".to_owned(),
            Leaf::WebSearch(_) => "tools.web_search".to_owned(),
            Leaf::Browser(_) => "tools.browser".to_owned(),
            Leaf::Counts(_) => "privacy.counts".to_owned(),
            Leaf::CrashReports(_) => "privacy.crash_reports".to_owned(),
            Leaf::Refused(key) => key.clone(),
        }
    }

    /// Whether this is a value, not a [`Leaf::Refused`] key.
    pub(crate) fn is_value(&self) -> bool {
        !matches!(self, Leaf::Refused(_))
    }

    /// Whether this turns on a key a policy may only turn off ([`OFF_ONLY_KEYS`]): a policy
    /// value that `Effective::load` ignores, with a warning, and does not lock.
    pub(crate) fn turns_on_an_off_only_key(&self) -> bool {
        let on = matches!(self, Leaf::Counts(true) | Leaf::CrashReports(true));
        debug_assert!(!on || OFF_ONLY_KEYS.contains(&self.key().as_str()));
        on
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
            Leaf::Lite(lite) => settings.ui.lite = lite,
            Leaf::Motion(motion) => settings.ui.motion = motion,
            Leaf::WebSearch(on) => settings.tools.web_search = on,
            Leaf::Browser(on) => settings.tools.browser = on,
            Leaf::Counts(on) => settings.privacy.counts = on,
            Leaf::CrashReports(on) => settings.privacy.crash_reports = on,
            Leaf::Refused(key) => reset(&key, settings),
        }
    }

    /// [`Leaf::apply`] for a policy source's value: a refused security key is locked off with
    /// [`refuse`], not put back at its default; any other leaf applies as from `settings.json`.
    pub(crate) fn apply_policy(self, settings: &mut Settings) {
        match self {
            Leaf::Refused(key) if refuse(&key, settings) => {}
            leaf => leaf.apply(settings),
        }
    }
}

/// Locks each security key `key` names at its refused value: `mcp.servers` empty, each
/// `tools` and `privacy` key `false`. Kept apart from [`reset`], which restores the default,
/// since a security key's default can be on. `key` is one of [`SECURITY_KEYS`], or a prefix
/// ending in `.` (`tools.`) that names every security key under it; stage C adds keys under
/// `plans.`.
/// Returns whether `key` named any security key; when it did not, nothing changes.
pub(crate) fn refuse(key: &str, settings: &mut Settings) -> bool {
    let named = |security: &str| match key.strip_suffix('.') {
        Some(_) => security.starts_with(key),
        None => security == key,
    };
    let mut any = false;
    for security in SECURITY_KEYS.into_iter().filter(|security| named(security)) {
        any = true;
        match security {
            "mcp.servers" => settings.mcp.servers.clear(),
            "tools.web_search" => settings.tools.web_search = false,
            "tools.browser" => settings.tools.browser = false,
            "privacy.counts" => settings.privacy.counts = false,
            "privacy.crash_reports" => settings.privacy.crash_reports = false,
            other => unreachable!("security key {other} has no refused value"),
        }
    }
    any
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
        "ui.lite" => settings.ui.lite = Lite::default(),
        "ui.motion" => settings.ui.motion = Motion::default(),
        "tools.web_search" => settings.tools.web_search = ToolsSettings::default().web_search,
        "tools.browser" => settings.tools.browser = ToolsSettings::default().browser,
        "privacy.counts" => settings.privacy.counts = PrivacySettings::default().counts,
        "privacy.crash_reports" => {
            settings.privacy.crash_reports = PrivacySettings::default().crash_reports;
        }
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
/// list empty). Inside it, an entry with a bad name, the reserved name [`RESERVED_MCP_SERVER`], a
/// value that does not type or a relative `command` is left out with a warning in `dropped`, and
/// the other entries load.
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
        if name.eq_ignore_ascii_case(RESERVED_MCP_SERVER) {
            dropped.push(WarningKind::InvalidValue {
                key: entry_key,
                message: format!(
                    "the server name {RESERVED_MCP_SERVER} is reserved for Clauseo's own browser"
                ),
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
