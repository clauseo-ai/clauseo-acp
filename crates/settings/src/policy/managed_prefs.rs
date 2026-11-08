//! macOS managed preferences for `ai.clauseo.desktop`, as an MDM configuration profile pushes
//! them.
//!
//! A value counts as policy only when it is forced (`CFPreferencesAppValueIsForced`): the same
//! call also returns what a user wrote with `defaults write`, which is not policy. Keys are the
//! dotted keys, except agents and plans: the key `agents` holds a dictionary from agent id to
//! launch spec, and `plans` one from agent id to plan. An `agents.<id>` or `plans.<id>` key is
//! unknown there.
//!
//! The shim asks for every key a setting can have and every other key the domain's managed
//! preferences files hold ([`read_with`], [`names_to_ask`]), so a misnamed key a firm forced
//! (`skill.extra`, or a `skills` dictionary holding `extra`) is seen: [`parse`] runs the
//! misnamed-section rule over it as for `policy.json` and the registry, nested dictionaries and
//! arrays included, and locks the section it stands for (host's review of #450).

#[cfg(test)]
mod tests;

use super::PolicyRead;
use crate::error::Error;
use crate::files::Files;
use crate::leaf::{self, Places, Raw};
use crate::warning::{Origin, Warning, WarningKind};
use serde_json::{Map, Number, Value};
use std::path::{Path, PathBuf};

/// The name the managed preferences source goes by in warnings and the policy status.
pub const SOURCE: &str = "managed preferences";

/// The key whose dictionary holds every agent.
pub const AGENTS_KEY: &str = "agents";

/// The key whose dictionary holds every agent's plan.
pub const PLANS_KEY: &str = "plans";

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

/// The keys the shim asks for: every key a setting can have, `agents` and `plans`, then each
/// other name in `listed` (what the domain lists, managed or not), once, sorted. Asking is
/// cheap and only a forced value counts, so a name listed by a layer that is not policy costs
/// nothing.
pub fn names_to_ask(listed: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut names: Vec<String> = leaf::KEYS
        .iter()
        .map(|&(key, _)| key)
        .chain([AGENTS_KEY, PLANS_KEY])
        .map(str::to_owned)
        .collect();
    let mut others: Vec<String> = listed
        .into_iter()
        .filter(|name| !names.contains(name))
        .collect();
    others.sort();
    others.dedup();
    names.extend(others);
    names
}

/// Where an MDM configuration profile's managed preferences land, and the only place a forced
/// value comes from: `<domain>.plist` for the computer, `<user>/<domain>.plist` for a user.
pub const MANAGED_DIR: &str = "/Library/Managed Preferences";

/// The managed preferences files of `domain`: the computer's, then the user `user`'s. A name
/// that is no plain folder name (empty, `.`, `..`, or holding a slash) is an error.
pub fn file_paths(domain: &str, user: &str) -> Result<Vec<PathBuf>, Error> {
    let dir = Path::new(MANAGED_DIR);
    if user.is_empty() || user == "." || user == ".." || user.contains(['/', '\\']) {
        return Err(no_user("the user name is no plain folder name"));
    }
    let file = format!("{domain}.plist");
    Ok(vec![dir.join(&file), dir.join(user).join(&file)])
}

/// The login user's name, from the real user id (`getpwuid_r`), never from the environment,
/// which the lawyer controls: `None` when the id has no user.
#[cfg(unix)]
pub fn login_user() -> std::io::Result<Option<String>> {
    use nix::unistd::{User, getuid};
    Ok(User::from_uid(getuid())?.map(|user| user.name))
}

/// [`file_paths`] for the user `looked_up` found ([`login_user`]); an error when the lookup
/// failed or found no user, so the source is unreadable and every security key locks off.
pub fn user_paths(
    domain: &str,
    looked_up: std::io::Result<Option<String>>,
) -> Result<Vec<PathBuf>, Error> {
    match looked_up {
        Ok(Some(user)) => file_paths(domain, &user),
        Ok(None) => Err(no_user("the real user id has no user")),
        Err(source) => Err(Error::Io {
            path: PathBuf::from(MANAGED_DIR),
            source,
        }),
    }
}

/// The error for a login user whose managed preferences file cannot be found.
fn no_user(why: &str) -> Error {
    Error::Io {
        path: PathBuf::from(MANAGED_DIR),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, why.to_owned()),
    }
}

/// [`read_with`] over the managed preferences files of `domain` for the user `looked_up` found
/// ([`user_paths`]); unreadable when there is no such user.
pub fn read_for(
    files: &dyn Files,
    domain: &str,
    looked_up: std::io::Result<Option<String>>,
    ask: impl Fn(Vec<String>) -> Vec<(String, PlistValue, bool)>,
) -> PolicyRead {
    match user_paths(domain, looked_up) {
        Ok(paths) => read_with(files, &paths, ask),
        Err(error) => PolicyRead::Unreadable(error),
    }
}

/// Reads managed preferences: the key names the files at `paths` hold ([`file_names`]), then
/// `ask` for each of [`names_to_ask`] (its value and whether it is forced, as the store says),
/// then [`parse`]. A file that cannot be read, or is no property list dictionary, makes the
/// source unreadable: a misnamed key in it would go unseen, so every security key locks off.
pub fn read_with(
    files: &dyn Files,
    paths: &[PathBuf],
    ask: impl Fn(Vec<String>) -> Vec<(String, PlistValue, bool)>,
) -> PolicyRead {
    match file_names(files, paths) {
        Ok(listed) => parse(ask(names_to_ask(listed))),
        Err(error) => PolicyRead::Unreadable(error),
    }
}

/// Every key name the managed preferences files at `paths` hold, read through `files` (a
/// symlink is followed). A missing file adds nothing; one that cannot be read is an
/// [`Error::Io`], and one that is no property list dictionary (XML or binary) an
/// [`Error::Parse`] at line and column 0, naming no content.
pub fn file_names(files: &dyn Files, paths: &[PathBuf]) -> Result<Vec<String>, Error> {
    let mut names = Vec::new();
    for path in paths {
        let bytes = match files.read(path) {
            Ok(None) => continue,
            Ok(Some(bytes)) => bytes,
            Err(source) => {
                return Err(Error::Io {
                    path: path.clone(),
                    source,
                });
            }
        };
        let keys = plist::Value::from_reader(std::io::Cursor::new(bytes))
            .ok()
            .and_then(plist::Value::into_dictionary)
            .ok_or_else(|| Error::Parse {
                path: path.clone(),
                line: 0,
                column: 0,
                message: "not a property list dictionary".to_owned(),
            })?;
        names.extend(keys.into_iter().map(|(key, _)| key));
    }
    Ok(names)
}

/// The forced values, typed, in the order given, which is each one's place
/// ([`crate::LockReason::place`]). `Absent` when none of them is forced. A forced key no
/// setting has, and each name nested in its dictionaries, goes through the misnamed-section
/// rule (`leaf::typed_policy`).
pub fn parse(values: Vec<(String, PlistValue, bool)>) -> PolicyRead {
    let forced: Vec<_> = values
        .into_iter()
        .filter_map(|(key, value, forced)| forced.then_some((key, value)))
        .collect();
    if forced.is_empty() {
        return PolicyRead::Absent;
    }
    let origin = Origin::Store {
        name: SOURCE.to_owned(),
    };
    let mut entries = Vec::new();
    let mut wrapped = Vec::new();
    let mut places = Places::default();
    for (at, (key, value)) in forced.into_iter().enumerate() {
        let place = at + 1;
        match value {
            PlistValue::Dict(by_id) if key == AGENTS_KEY || key == PLANS_KEY => entries.extend(
                by_id
                    .into_iter()
                    .map(|(id, entry)| (format!("{key}.{id}"), Some(entry), place)),
            ),
            value if leaf::KEYS.iter().any(|&(known, _)| known == key) => {
                entries.push((key, Some(value), place));
            }
            value => {
                nested_names(&key, &value, &origin, &mut wrapped);
                places.wrapped.resize(wrapped.len(), place);
                entries.push((key, None, place));
            }
        }
    }
    let mut raws = Vec::new();
    let mut refused = Vec::new();
    let mut warnings = Vec::new();
    for (key, value, place) in entries {
        let kind = match value.map(plain_json) {
            Some(_) if key.starts_with("plans.") && crate::plans::plan_id(&key).is_none() => {
                // A plan under a name that is no agent id, whatever its value (a date or data
                // too): what the firm meant is unknown, so no plan stands, as for a `plans`
                // that is no dictionary.
                refused.push("plans.".to_owned());
                WarningKind::InvalidValue {
                    key: PLANS_KEY.to_owned(),
                    message: crate::plans::BAD_ID.to_owned(),
                }
            }
            Some(Ok(value)) => {
                places.raws.push(place);
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
            None if key == PLANS_KEY => {
                // What the firm meant is unknown, so no plan stands (decision record 0048).
                refused.push("plans.".to_owned());
                WarningKind::InvalidValue {
                    key,
                    message: "it must be a dictionary from agent id to plan".to_owned(),
                }
            }
            None => WarningKind::UnknownKey { key },
        };
        places.warnings.push(place);
        warnings.push(Warning {
            origin: origin.clone(),
            kind,
        });
    }
    PolicyRead::Read(leaf::typed_policy(
        raws, &wrapped, refused, warnings, &places,
    ))
}

/// Pushes `prefix.<key>` for each key of `value`, when it is a dictionary, then the same for
/// each value under it, at every depth; an array's items are walked under `prefix` itself. As
/// [`crate::json`] does for an unknown `policy.json` section.
fn nested_names(prefix: &str, value: &PlistValue, origin: &Origin, names: &mut Vec<Raw>) {
    let entries = match value {
        PlistValue::Dict(entries) => entries,
        PlistValue::Array(items) => {
            for item in items {
                nested_names(prefix, item, origin, names);
            }
            return;
        }
        _ => return,
    };
    for (key, value) in entries {
        let name = format!("{prefix}.{key}");
        nested_names(&name, value, origin, names);
        names.push(Raw {
            key: name,
            value: Value::Null,
            origin: origin.clone(),
        });
    }
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
