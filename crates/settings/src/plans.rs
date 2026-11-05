//! The rules of the `plans` and `install` keys (decision record 0048): a firm plan's fields, the
//! variables a plan may set, the web addresses an endpoint or a mirror may be, and which agent
//! entries are Clauseo's own to replace.
//!
//! Every message here is fixed text, a variable's name or a variable's place (`env[2]`): none
//! quotes a value, so an endpoint, a mirror or a variable's value never reaches a warning, an
//! error or a log. A name that is no variable name is never quoted either, since it can be
//! `NAME=value` text.

#[cfg(test)]
mod tests;

use crate::error::Error;
use crate::schema::{AgentLaunch, Plan};
use crate::warning::WarningKind;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

/// The fields a [`Plan`] reads; any other is an unknown key.
const PLAN_FIELDS: [&str; 3] = ["endpoint", "provider", "env"];

/// The variables a plan's `env` may set (decision record 0048), each by its exact name. Anything
/// else is refused: a credential can hide under any name (`ANTHROPIC_CUSTOM_HEADERS`,
/// `DB_PASSWD`), so only names Clauseo supports pass. No `*_BASE_URL` and no
/// `CLAUDE_CODE_USE_*` is here: the plan's endpoint and provider set those, under the https rule.
pub const PLAN_ENV_NAMES: [&str; 7] = [
    "AWS_REGION",
    "AWS_PROFILE",
    "CLOUD_ML_REGION",
    "ANTHROPIC_VERTEX_PROJECT_ID",
    "ANTHROPIC_MODEL",
    "ANTHROPIC_SMALL_FAST_MODEL",
    "NODE_EXTRA_CA_CERTS",
];

/// The proxy variables a plan's `env` may also set, each in any case. A value may not hold a
/// user name or password (`@`).
pub const PLAN_PROXY_NAMES: [&str; 3] = ["HTTPS_PROXY", "HTTP_PROXY", "NO_PROXY"];

/// The words a variable name may not hold, in any case, where a name is free (an agent's launch
/// `env` with a value, [`crate::Effective::set_agent_launch`]).
pub const SECRET_WORDS: [&str; 4] = ["KEY", "TOKEN", "SECRET", "PASSWORD"];

/// Why a secret-shaped variable name is refused.
const SECRET_NAME: &str =
    "a variable name holding KEY, TOKEN, SECRET or PASSWORD is refused: keys go in the OS keychain";

/// Why a variable name that is not one is refused.
const BAD_NAME: &str = "a variable name is ASCII letters, digits and _, not starting with a digit";

/// Why a variable the plan's endpoint or provider sets is refused in its `env`.
const ROUTED_NAME: &str = "the plan's endpoint and provider set this variable, not its env";

/// Why any other variable is refused in a plan's `env`.
const NOT_ALLOWED: &str = "a plan may set only AWS_REGION, AWS_PROFILE, CLOUD_ML_REGION, \
                           ANTHROPIC_VERTEX_PROJECT_ID, ANTHROPIC_MODEL, \
                           ANTHROPIC_SMALL_FAST_MODEL, NODE_EXTRA_CA_CERTS and the proxy variables";

/// Why a proxy address with a user name or password is refused.
const PROXY_LOGIN: &str =
    "a proxy address may not hold a user name or password (@): keys go in the OS keychain";

/// Why a launch variable with a secret-shaped name and a value is refused.
const SECRET_VALUE: &str = "a variable whose name holds KEY, TOKEN, SECRET or PASSWORD must be \
                            empty here: keys go in the OS keychain";

/// Why a plan id that is not an agent id is refused.
pub(crate) const BAD_ID: &str = "a plan's agent id is 1 to 64 characters of [a-z0-9-]";

/// Why an endpoint or a mirror is refused.
const BAD_ADDRESS: &str = "it must be an https:// URL in ASCII (punycode for a foreign host) with \
                           a host, a port of at most 65535 if any, and no user name, password, \
                           query, fragment or backslash";

/// Why a plan that does not type is refused. Fixed, since serde's own message can quote a value.
const BAD_PLAN: &str = "it must be an object with an endpoint, a provider (anthropic, bedrock or \
                        vertex) and env, an object of text values";

/// The agent id in a `plans.<id>` key: an agent id's characters only ([`id_ok`]), so a dotted
/// name such as `plans.claude-acp.endpoint` is no plan key, and a policy that names one locks
/// every plan as a misnamed key.
pub(crate) fn plan_id(key: &str) -> Option<&str> {
    key.strip_prefix("plans.").filter(|id| id_ok(id))
}

/// Whether `id` is an agent id as `protocol`'s `AgentId` checks it: 1 to 64 characters of
/// `[a-z0-9-]`.
pub(crate) fn id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Whether a plan variable's `name` is secret-shaped: it holds one of [`SECRET_WORDS`], ignoring
/// ASCII case. A plan refuses it as it refuses every name not on its list, with this reason;
/// [`crate::Effective::set_agent_launch`] refuses it with a value.
pub fn secret_shaped(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    SECRET_WORDS.iter().any(|word| upper.contains(word))
}

/// Whether `name` is a variable name: ASCII letters, digits and `_`, not starting with a digit.
fn name_shaped(name: &str) -> bool {
    name.bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && name
            .bytes()
            .next()
            .is_some_and(|first| !first.is_ascii_digit())
}

/// Whether an unknown field `text` may be quoted: `[a-z][a-z0-9_]{0,31}`, which still shows a
/// typo (`envv`, `endpoit`). Anything else may be pasted text (`NAME=value`, a `ghp_…` or other
/// token, which mixes case or runs long), so a message gives its place instead.
pub(crate) fn field_shaped(text: &str) -> bool {
    text.len() <= 32
        && text
            .bytes()
            .next()
            .is_some_and(|first| first.is_ascii_lowercase())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

/// How a message shows the key `key`, the `place`th `plans.` entry of its source (from 1): as
/// written when its id passes [`id_ok`] and each dotted part after the id is
/// [`field_shaped`] (so a misnamed `plans.claude-acp.endpoint` still reads as itself), and as
/// `plans.<id {place}>` otherwise. Any key outside `plans.` is as written.
pub(crate) fn shown_plan_key(key: &str, place: usize) -> String {
    let Some(rest) = key.strip_prefix("plans.") else {
        return key.to_owned();
    };
    let mut parts = rest.split('.');
    if parts.next().is_some_and(id_ok) && parts.all(field_shaped) {
        key.to_owned()
    } else {
        format!("plans.<id {place}>")
    }
}

/// Whether `name` is one of [`PLAN_PROXY_NAMES`], in any case.
fn proxy_name(name: &str) -> bool {
    PLAN_PROXY_NAMES
        .iter()
        .any(|proxy| proxy.eq_ignore_ascii_case(name))
}

/// Why the variable `name` set to `value` may not be a plan variable, or `None` when it may.
fn refused_variable(name: &str, value: &str) -> Option<&'static str> {
    let upper = name.to_ascii_uppercase();
    if !name_shaped(name) {
        Some(BAD_NAME)
    } else if PLAN_ENV_NAMES.contains(&name) {
        None
    } else if proxy_name(name) {
        value.contains('@').then_some(PROXY_LOGIN)
    } else if upper.ends_with("_BASE_URL") || upper.starts_with("CLAUDE_CODE_USE_") {
        Some(ROUTED_NAME)
    } else if secret_shaped(name) {
        Some(SECRET_NAME)
    } else {
        Some(NOT_ALLOWED)
    }
}

/// The key a message gives for the variable `name`, the `index`th of `section`'s `env` in
/// name order: `<section>.env.<name>` for a variable name, which holds no value, and
/// `<section>.env[<index>]` for anything else, which may be `NAME=value` text.
fn env_key(section: &str, index: usize, name: &str) -> String {
    if name_shaped(name) {
        format!("{section}.env.{name}")
    } else {
        format!("{section}.env[{index}]")
    }
}

/// The first variable of `env` that `refused` refuses, as its key ([`env_key`]) and why.
fn first_refused(
    section: &str,
    env: &BTreeMap<String, String>,
    refused: impl Fn(&str, &str) -> Option<&'static str>,
) -> Option<(String, &'static str)> {
    env.iter().enumerate().find_map(|(index, (name, value))| {
        refused(name, value).map(|reason| (env_key(section, index, name), reason))
    })
}

/// Whether `url` is a web address an endpoint or a mirror may be: ASCII only (a foreign host in
/// punycode, so no right-to-left override or zero-width character), `https://` in any case,
/// then a host and a port of at most 65535 if any, with no user name or password (`@`), query
/// (`?`), fragment (`#`), backslash, space or control character anywhere.
pub fn web_address_ok(url: &str) -> bool {
    let Some(rest) = url
        .get(..8)
        .filter(|scheme| url.is_ascii() && scheme.eq_ignore_ascii_case("https://"))
        .and_then(|_| url.get(8..))
    else {
        return false;
    };
    authority_ok(rest.split('/').next().unwrap_or_default())
        && !url.chars().any(|c| {
            matches!(c, '@' | '?' | '#' | '\\') || c.is_ascii_whitespace() || c.is_ascii_control()
        })
}

/// Whether `authority` is a host, or `[` an IPv6 literal `]`, then `:` and a port of 1 to 5
/// digits up to 65535 if any.
fn authority_ok(authority: &str) -> bool {
    let (host, port) = match authority.strip_prefix('[') {
        Some(literal) => match literal.split_once(']') {
            Some((inside, "")) => (inside, None),
            Some((inside, after)) => match after.strip_prefix(':') {
                Some(port) => (inside, Some(port)),
                None => return false,
            },
            None => return false,
        },
        None => match authority.rsplit_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        },
    };
    !host.is_empty()
        && port.is_none_or(|port| {
            port.bytes().all(|byte| byte.is_ascii_digit()) && port.parse::<u16>().is_ok()
        })
}

/// Types the `plans.<id>` value. A value that does not type, or an endpoint that is not a web
/// address, refuses the entry. Each variable a plan may not set ([`PLAN_ENV_NAMES`],
/// [`PLAN_PROXY_NAMES`]) is left out with a warning in `dropped`, naming it (or giving its place,
/// for a name that is no variable name), and the rest of the plan loads; so does each field this
/// version does not know, quoted only when it is a plain word (`plans.claude-acp.<field 2>`
/// otherwise).
pub(crate) fn plan(
    key: &str,
    value: Value,
    dropped: &mut Vec<WarningKind>,
) -> Result<Plan, WarningKind> {
    let refused = |message: &str| WarningKind::InvalidValue {
        key: key.to_owned(),
        message: message.to_owned(),
    };
    if let Value::Object(fields) = &value {
        // A field is quoted only when [`field_shaped`]; else by its place, from 1.
        dropped.extend(
            fields
                .keys()
                .enumerate()
                .filter(|(_, field)| !PLAN_FIELDS.contains(&field.as_str()))
                .map(|(at, field)| WarningKind::UnknownKey {
                    key: if field_shaped(field) {
                        format!("{key}.{field}")
                    } else {
                        format!("{key}.<field {}>", at + 1)
                    },
                }),
        );
    }
    // serde's message can quote the value, so it is dropped for the fixed one.
    let mut plan: Plan = serde_json::from_value(value)
        .ok()
        .ok_or_else(|| refused(BAD_PLAN))?;
    if plan
        .endpoint
        .as_deref()
        .is_some_and(|url| !web_address_ok(url))
    {
        return Err(WarningKind::InvalidValue {
            key: format!("{key}.endpoint"),
            message: BAD_ADDRESS.to_owned(),
        });
    }
    let mut index = 0;
    plan.env.retain(|name, value| {
        let at = index;
        index += 1;
        match refused_variable(name, value) {
            Some(message) => {
                dropped.push(WarningKind::InvalidValue {
                    key: env_key(key, at, name),
                    message: message.to_owned(),
                });
                false
            }
            None => true,
        }
    });
    Ok(plan)
}

/// [`plan`]'s rules for a runtime write of `plans.<id>`: the first field that breaks one, as
/// [`Error::Refused`]. Nothing is left out here: the whole write is refused.
pub(crate) fn check_plan(id: &str, plan: &Plan) -> Result<(), Error> {
    if plan
        .endpoint
        .as_deref()
        .is_some_and(|url| !web_address_ok(url))
    {
        return Err(Error::Refused {
            key: format!("plans.{id}.endpoint"),
            reason: BAD_ADDRESS,
        });
    }
    match first_refused(&format!("plans.{id}"), &plan.env, refused_variable) {
        Some((key, reason)) => Err(Error::Refused { key, reason }),
        None => Ok(()),
    }
}

/// The rule [`crate::Effective::set_agent_launch`] holds a launch's `env` to: a secret-shaped
/// name ([`secret_shaped`]) is refused with a value, since keys live only in the keychain (the
/// empty placeholder a stored key replaces at launch passes). The refusal names the key, never
/// the value.
pub(crate) fn check_launch_env(id: &str, launch: &AgentLaunch) -> Result<(), Error> {
    let refused = |name: &str, value: &str| {
        (secret_shaped(name) && !value.is_empty()).then_some(SECRET_VALUE)
    };
    match first_refused(&format!("agents.{id}"), &launch.env, refused) {
        Some((key, reason)) => Err(Error::Refused { key, reason }),
        None => Ok(()),
    }
}

/// Types `install.bundle`: an absolute path, or `null` for unset.
pub(crate) fn bundle(key: &str, value: Value) -> Result<Option<PathBuf>, WarningKind> {
    match value {
        Value::Null => Ok(None),
        Value::String(path) if Path::new(&path).is_absolute() => Ok(Some(PathBuf::from(path))),
        _ => Err(WarningKind::InvalidValue {
            key: key.to_owned(),
            message: "it must be an absolute path, or null".to_owned(),
        }),
    }
}

/// Types `install.registry`: a web address ([`web_address_ok`]), or `null` for unset.
pub(crate) fn registry(key: &str, value: Value) -> Result<Option<String>, WarningKind> {
    match value {
        Value::Null => Ok(None),
        Value::String(url) if web_address_ok(&url) => Ok(Some(url)),
        _ => Err(WarningKind::InvalidValue {
            key: key.to_owned(),
            message: format!("{BAD_ADDRESS}, or null"),
        }),
    }
}

/// Whether an `agents.<id>` value, as `settings.json` holds it, is Clauseo's own ([`owns`]).
/// Anything else, an entry that does not type included, is the lawyer's or the firm's, and
/// Clauseo never replaces it.
pub(crate) fn entry_is_clauseos(entry: &Value, data_dir: &Path) -> bool {
    let Some(command) = entry.get("command").and_then(Value::as_str) else {
        return false;
    };
    let script = entry
        .get("args")
        .and_then(Value::as_array)
        .and_then(|args| args.first())
        .and_then(Value::as_str);
    owns(command, script, data_dir)
}

/// [`entry_is_clauseos`] for a launch about to be written.
pub(crate) fn launch_is_clauseos(launch: &AgentLaunch, data_dir: &Path) -> bool {
    owns(
        &launch.command,
        launch.args.first().map(String::as_str),
        data_dir,
    )
}

/// Whether a launch of `command` with first argument `script` is Clauseo's install: `command` is
/// under `<data_dir>/agents/`, or `command` is an absolute path to a `node` or `node.exe` (the
/// lawyer's own Node: Homebrew, nvm, the system's) and `script` is under `<data_dir>/agents/`.
/// Any other program with an adapter's script as its argument (`/bin/sh`) is not.
fn owns(command: &str, script: Option<&str>, data_dir: &Path) -> bool {
    let command = Path::new(command);
    let node = command.is_absolute()
        && command
            .file_name()
            .is_some_and(|name| name == "node" || name == "node.exe");
    under_agents(command, data_dir)
        || (node && script.is_some_and(|script| under_agents(Path::new(script), data_dir)))
}

/// Whether `path` is absolute, has no `..`, and lies under `<data_dir>/agents/`, compared
/// component by component as written: a path spelled another way counts as outside, which
/// only leaves an entry alone.
fn under_agents(path: &Path, data_dir: &Path) -> bool {
    path.is_absolute()
        && !path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
        && path.starts_with(data_dir.join("agents"))
        && path != data_dir.join("agents")
}
