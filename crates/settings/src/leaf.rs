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
//!   in `reset` (`leaf/reset.rs`), which a value that does not type puts the key back at its
//!   default (`leaf/tests.rs`' `a_refused_key_goes_back_to_its_default` fails for a row with no
//!   arm);
//! - for a `ui` key, its field in `UiPatch` (`ui.rs`, and its name in `UiPatch::keys`) and in
//!   `Settings::apply_ui_patch`. Two tests in `ui/tests.rs` fail when `keys()` misses a `ui`
//!   key or differs from the names the writer saves: `the_keys_are_the_names_the_writer_saves`
//!   and `the_keys_a_patch_sets_are_the_ui_policy_keys_in_field_order`;
//! - a sample value in `leaf/tests.rs`' `every_fixed_key_types_and_the_kinds_agree`, and a
//!   value that is not the default in `a_refused_key_goes_back_to_its_default`;
//! - for a key that decides what an agent can reach, or what Clauseo sends off the machine, its
//!   name in [`SECURITY_KEYS`] (a section of entries by id, as `plans.`, ends in `.`) and its
//!   arm in [`refuse`] (`leaf/reset.rs`), which sets the value it is locked at when its policy
//!   is unknown;
//! - for a key a policy may only turn off, its name in [`OFF_ONLY_KEYS`] (`Effective::load`
//!   ignores a policy `true` for it, with a warning);
//! - for a key only a policy may set, its name in [`POLICY_ONLY_KEYS`] (`settings.json`'s value
//!   is ignored, with a warning).
//!
//! `schema::tests::the_keys_and_their_kinds_are_the_schemas` fails when the schema and [`KEYS`]
//! disagree, and the two samples tests fail when [`KEYS`] has a row without a sample.

#[cfg(test)]
mod mcp_tests;
mod misnamed;
mod reset;
#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod tests;

pub(crate) use misnamed::{Places, typed_policy};

use crate::plans::{self, plan_id};
use crate::policy::{Kind, kind_of};
use crate::schema::{
    AgentLaunch, Density, FontSize, Language, Lite, McpServerLaunch, Motion, Plan, Settings,
    SkillsExtra, Theme, UpdateMode,
};
use crate::skills;
use crate::warning::{Origin, Warning, WarningKind};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub(crate) use reset::refuse;
use reset::reset;

/// Every key with a fixed name, and its kind. An agent's key is `agents.<id>`, and a plan's
/// `plans.<id>`, each an object. `schema::tests::the_keys_and_their_kinds_are_the_schemas`
/// holds this to the schema.
pub(crate) const KEYS: [(&str, Kind); 19] = [
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
    ("update.mode", Kind::String),
    ("install.bundle", Kind::String),
    ("install.registry", Kind::String),
    ("skills.extra", Kind::String),
    ("skills.curated", Kind::StringArray),
    ("skills.allow_scripts", Kind::Boolean),
    ("skills.enabled", Kind::Object),
];

/// The keys that decide what an agent can reach, what Clauseo sends off the machine, or what it
/// fetches and runs. A policy source that cannot be read locks each off with [`refuse`] (ADR
/// 0009, R21; the `privacy` keys, decision record 0036; `update.mode` at `none`, R23): its value
/// there is unknown, and the user's must not stand in. Off is not always the default: both
/// `tools` keys, `privacy.counts` and `update.mode` default to on, and locking them at their
/// default would fail open. `plans.` stands for every `plans.<id>` (decision record 0048):
/// locked, no plan stands, so no endpoint the lawyer chose does. The three `skills` keys lock
/// at no extra pack, none from a list, and no script (decision record 0040).
pub(crate) const SECURITY_KEYS: [&str; 12] = [
    "mcp.servers",
    "tools.web_search",
    "tools.browser",
    "privacy.counts",
    "privacy.crash_reports",
    "update.mode",
    "install.bundle",
    "install.registry",
    "plans.",
    "skills.extra",
    "skills.allow_scripts",
    "skills.curated",
];

/// The keys a policy may only turn off (decision record 0036): a policy `true` for one is
/// ignored with a warning and does not lock it, so the lawyer's own switch stands. A policy
/// `false` locks it, as any policy value does.
pub(crate) const OFF_ONLY_KEYS: [&str; 2] = ["privacy.counts", "privacy.crash_reports"];

/// The keys only a policy may set (decision record 0040): a `settings.json` value for one is
/// ignored with a warning ([`without_policy_only`]) and never applies, so without a policy
/// `skills.curated` is empty and admits nothing.
pub(crate) const POLICY_ONLY_KEYS: [&str; 1] = ["skills.curated"];

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
    UpdateMode(UpdateMode),
    Plan(String, Plan),
    Bundle(Option<PathBuf>),
    Registry(Option<String>),
    SkillsExtra(SkillsExtra),
    Curated(Vec<String>),
    AllowScripts(bool),
    /// The whole `skills.enabled` map: a policy value replaces the user's map, never merges.
    SkillsEnabled(BTreeMap<String, String>),
    /// A known key whose value does not type, or that a policy source refused. Applied, it puts
    /// the key back at its default; for `agents.<id>` or `plans.<id>`, that entry is dropped. A
    /// policy also locks the key (fail closed, Rohan, 2026-10-31), and applies it with
    /// [`Leaf::apply_policy`], which locks a security key off rather than at its default.
    Refused(String),
}

/// The agent id in an `agents.<id>` key.
pub(crate) fn agent_id(key: &str) -> Option<&str> {
    key.strip_prefix("agents.").filter(|id| !id.is_empty())
}

/// `raws` from `settings.json` without the keys only a policy may set ([`POLICY_ONLY_KEYS`]):
/// each such value gets a warning where it sits, and is left out before it is typed.
pub(crate) fn without_policy_only(raws: Vec<Raw>, warnings: &mut Vec<Warning>) -> Vec<Raw> {
    let (ignored, kept): (Vec<Raw>, Vec<Raw>) = raws
        .into_iter()
        .partition(|raw| POLICY_ONLY_KEYS.contains(&raw.key.as_str()));
    warnings.extend(ignored.into_iter().map(|raw| Warning {
        kind: WarningKind::InvalidValue {
            message: format!(
                "only a policy can set {}, so this value is ignored and admits nothing",
                raw.key
            ),
            key: raw.key,
        },
        origin: raw.origin,
    }));
    kept
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
    let mut plans_seen = 0;
    for raw in raws {
        let mut kinds = Vec::new();
        plans_seen += usize::from(raw.key.starts_with("plans."));
        match Leaf::parse(&raw.key, raw.value, &mut kinds) {
            Ok(leaf) => leaves.push(leaf),
            Err(kind) => {
                if kind_of(&raw.key).is_some() {
                    leaves.push(Leaf::Refused(raw.key.clone()));
                }
                // A `plans.` key that is no plan's can be pasted text: shown by its place.
                kinds.push(match kind {
                    WarningKind::UnknownKey { key } => WarningKind::UnknownKey {
                        key: plans::shown_plan_key(&key, plans_seen),
                    },
                    other => other,
                });
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
            "update.mode" => typed_value(key, value).map(Leaf::UpdateMode),
            "install.bundle" => plans::bundle(key, value).map(Leaf::Bundle),
            "install.registry" => plans::registry(key, value).map(Leaf::Registry),
            "skills.extra" => typed_value(key, value).map(Leaf::SkillsExtra),
            "skills.curated" => skills::curated(key, value).map(Leaf::Curated),
            "skills.allow_scripts" => typed_value(key, value).map(Leaf::AllowScripts),
            "skills.enabled" => skills::enabled(key, value, unknown).map(Leaf::SkillsEnabled),
            _ => match (agent_id(key), plan_id(key)) {
                (Some(id), _) => agent(key, id, value, unknown),
                (_, Some(id)) => plans::plan(key, value, unknown).map(|p| Leaf::Plan(id.into(), p)),
                (None, None) => Err(WarningKind::UnknownKey {
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
            Leaf::UpdateMode(_) => "update.mode".to_owned(),
            Leaf::Plan(id, _) => format!("plans.{id}"),
            Leaf::Bundle(_) => "install.bundle".to_owned(),
            Leaf::Registry(_) => "install.registry".to_owned(),
            Leaf::SkillsExtra(_) => "skills.extra".to_owned(),
            Leaf::Curated(_) => "skills.curated".to_owned(),
            Leaf::AllowScripts(_) => "skills.allow_scripts".to_owned(),
            Leaf::SkillsEnabled(_) => "skills.enabled".to_owned(),
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

    /// Writes the value into `settings`, replacing what was there. An agent or a plan replaces
    /// the whole entry for its id.
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
            Leaf::UpdateMode(mode) => settings.update.mode = mode,
            Leaf::Plan(id, plan) => {
                settings.plans.insert(id, plan);
            }
            Leaf::Bundle(path) => settings.install.bundle = path,
            Leaf::Registry(url) => settings.install.registry = url,
            Leaf::SkillsExtra(extra) => settings.skills.extra = extra,
            Leaf::Curated(ids) => settings.skills.curated = ids,
            Leaf::AllowScripts(on) => settings.skills.allow_scripts = on,
            Leaf::SkillsEnabled(packs) => settings.skills.enabled = packs,
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
