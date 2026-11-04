//! The rules of the `plans` and `install` keys (decision record 0048): a firm plan's fields, the
//! refusal of a secret-shaped variable name, the web addresses an endpoint or a mirror may be,
//! and which agent entries are Clauseo's own to replace.
//!
//! Every message here is fixed text or a variable's name: none quotes a value, so an endpoint,
//! a mirror or a variable's value never reaches a warning, an error or a log.

#[cfg(test)]
mod tests;

use crate::error::Error;
use crate::schema::{AgentLaunch, Plan};
use crate::warning::WarningKind;
use serde_json::Value;
use std::path::{Component, Path, PathBuf};

/// The fields a [`Plan`] reads; any other is an unknown key.
const PLAN_FIELDS: [&str; 3] = ["endpoint", "provider", "env"];

/// The words a plan variable's name may not hold, in any case.
pub const SECRET_WORDS: [&str; 4] = ["KEY", "TOKEN", "SECRET", "PASSWORD"];

/// Why a secret-shaped variable name is refused.
const SECRET_NAME: &str =
    "a variable name holding KEY, TOKEN, SECRET or PASSWORD is refused: keys go in the OS keychain";

/// Why a variable name that is not one is refused.
const BAD_NAME: &str = "a variable name is ASCII letters, digits and _, not starting with a digit";

/// Why an endpoint or a mirror is refused.
const BAD_ADDRESS: &str =
    "it must be an https:// URL with a host, and no user name, password, query or fragment";

/// Why a plan that does not type is refused. Fixed, since serde's own message can quote a value.
const BAD_PLAN: &str = "it must be an object with an endpoint, a provider (anthropic, bedrock or \
                        vertex) and env, an object of text values";

/// The agent id in a `plans.<id>` key.
pub(crate) fn plan_id(key: &str) -> Option<&str> {
    key.strip_prefix("plans.").filter(|id| !id.is_empty())
}

/// Whether a plan variable's `name` is secret-shaped: it holds one of [`SECRET_WORDS`], ignoring
/// ASCII case. Such a name is refused in `settings.json`, in every policy source and in
/// [`crate::Effective::set_plan`].
pub fn secret_shaped(name: &str) -> bool {
    let upper = name.to_ascii_uppercase();
    SECRET_WORDS.iter().any(|word| upper.contains(word))
}

/// Why the variable `name` may not be a plan variable, or `None` when it may.
fn refused_name(name: &str) -> Option<&'static str> {
    let shaped = name
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && name
            .bytes()
            .next()
            .is_some_and(|first| !first.is_ascii_digit());
    if !shaped {
        Some(BAD_NAME)
    } else if secret_shaped(name) {
        Some(SECRET_NAME)
    } else {
        None
    }
}

/// Whether `url` is a web address an endpoint or a mirror may be: `https://` in any case, then
/// a host, with no user name or password (`@`), query (`?`), fragment (`#`), space or control
/// character anywhere.
pub(crate) fn web_address_ok(url: &str) -> bool {
    let Some(rest) = url
        .get(..8)
        .filter(|scheme| scheme.eq_ignore_ascii_case("https://"))
        .and_then(|_| url.get(8..))
    else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or_default();
    !host.is_empty()
        && !host.starts_with(':')
        && !url
            .chars()
            .any(|c| matches!(c, '@' | '?' | '#') || c.is_whitespace() || c.is_control())
}

/// Types the `plans.<id>` value. A value that does not type, or an endpoint that is not a web
/// address, refuses the entry. Each variable whose name is refused is left out with a warning
/// in `dropped`, naming it, and the rest of the plan loads; so does each field this version
/// does not know.
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
        dropped.extend(
            fields
                .keys()
                .filter(|field| !PLAN_FIELDS.contains(&field.as_str()))
                .map(|field| WarningKind::UnknownKey {
                    key: format!("{key}.{field}"),
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
    plan.env.retain(|name, _| match refused_name(name) {
        Some(message) => {
            dropped.push(WarningKind::InvalidValue {
                key: format!("{key}.env.{name}"),
                message: message.to_owned(),
            });
            false
        }
        None => true,
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
    match plan
        .env
        .keys()
        .find_map(|name| refused_name(name).map(|reason| (name, reason)))
    {
        Some((name, reason)) => Err(Error::Refused {
            key: format!("plans.{id}.env.{name}"),
            reason,
        }),
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

/// Whether an `agents.<id>` value, as `settings.json` holds it, is Clauseo's own: its `command`,
/// or its first argument (the adapter's script, when the command is the lawyer's own Node), is a
/// path under `<data_dir>/agents/`. Anything else, an entry that does not type included, is the
/// lawyer's or the firm's, and Clauseo never replaces it.
pub(crate) fn entry_is_clauseos(entry: &Value, data_dir: &Path) -> bool {
    let Some(command) = entry.get("command").and_then(Value::as_str) else {
        return false;
    };
    let script = entry
        .get("args")
        .and_then(Value::as_array)
        .and_then(|args| args.first())
        .and_then(Value::as_str);
    [Some(command), script]
        .into_iter()
        .flatten()
        .any(|path| under_agents(Path::new(path), data_dir))
}

/// [`entry_is_clauseos`] for a launch about to be written.
pub(crate) fn launch_is_clauseos(launch: &AgentLaunch, data_dir: &Path) -> bool {
    let script = launch.args.first().map(String::as_str);
    [Some(launch.command.as_str()), script]
        .into_iter()
        .flatten()
        .any(|path| under_agents(Path::new(path), data_dir))
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
