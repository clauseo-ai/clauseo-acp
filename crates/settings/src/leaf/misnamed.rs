//! The security keys a policy source names in a form this version cannot read (ADR 0009,
//! decision records 0036 and 0048): each is refused, so locked off, since what the firm meant
//! there is unknown and the user's value must not stand in.

use super::{Leaf, Raw, SECURITY_KEYS, typed};
use crate::policy::kind_of;
use crate::warning::{Origin, Warning, WarningKind};

/// [`typed`] for a policy source that lists its own keys (`policy.json` and the registry; macOS
/// managed preferences are read key by key, by exact name, so they use [`typed`]). A
/// security-relevant key is also refused, so locked off (ADR 0009, [`super::refuse`]), when the
/// source names its topic in a form this version cannot read: its section that is not an object
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

/// Whether the unknown `name` is the security `key` itself, or for a section of entries by id
/// (`plans.`) one of its keys. A top-level `"plans.claude-acp"` in `policy.json` sets nothing,
/// and its name is a plan key's, which `about` passes over.
fn names(name: &str, key: &str) -> bool {
    name == key || (key.ends_with('.') && name.starts_with(key))
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
                WarningKind::UnknownKey { key: name } if about(name) || names(name, key) => {
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
