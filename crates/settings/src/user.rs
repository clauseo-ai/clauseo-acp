//! The user's `settings.json`: reading it, and the write of a [`UiPatch`], a [`ToolsPatch`], a
//! [`PrivacyPatch`] or an [`UpdateChoice`], or of one `agents.<id>`, `plans.<id>` or
//! `skills.enabled` entry. Only [`crate::Effective::apply_ui_patch`],
//! [`crate::Effective::apply_tools_patch`], [`crate::Effective::apply_privacy_patch`],
//! [`crate::Effective::apply_update_mode`], [`crate::Effective::set_agent_launch`],
//! [`crate::Effective::set_plan`] and [`crate::Effective::set_skill_pack`] call those writes,
//! after they have checked policy locks.

#[cfg(test)]
mod tests;

use crate::error::Error;
use crate::files::Files;
use crate::json;
use crate::leaf::{self, Leaf};
use crate::locations::Locations;
use crate::privacy::PrivacyPatch;
use crate::tools::ToolsPatch;
use crate::ui::UiPatch;
use crate::update::{UpdateChoice, UpdateFields};
use crate::warning::{Origin, Warning, WarningKind};
use serde::Serialize;
use serde_json_lenient::{Map, Value};
use std::path::Path;

/// The user's values, one per key, in the order of each key's last occurrence: a key the file
/// sets twice keeps its last value, valid or not. A missing file holds none. A file that cannot
/// be read or parsed holds none either, with a warning: a broken file never stops the app from
/// starting.
pub(crate) fn read(path: &Path, files: &dyn Files, warnings: &mut Vec<Warning>) -> Vec<Leaf> {
    let whole_file = |kind| Warning {
        origin: Origin::File {
            path: path.to_owned(),
            position: None,
        },
        kind,
    };
    let bytes = match files.read(path) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Vec::new(),
        Err(error) => {
            warnings.push(whole_file(WarningKind::Unreadable {
                message: error.to_string(),
            }));
            return Vec::new();
        }
    };
    match json::read(path, &bytes) {
        Ok((raws, file_warnings, _wrapped, _places)) => {
            warnings.extend(file_warnings);
            // A key twice in one object: the last copy wins, with a warning at the second.
            if let Ok(Some(position)) = json::duplicate(&bytes) {
                warnings.push(Warning {
                    origin: Origin::File {
                        path: path.to_owned(),
                        position: Some(position),
                    },
                    kind: WarningKind::DuplicateKey,
                });
            }
            let raws = leaf::without_policy_only(raws, warnings);
            leaf::typed(raws, Vec::new(), warnings)
        }
        Err(syntax) => {
            warnings.push(Warning {
                origin: Origin::File {
                    path: path.to_owned(),
                    position: Some(syntax.position),
                },
                kind: WarningKind::Syntax {
                    message: syntax.message,
                },
            });
            Vec::new()
        }
    }
}

/// Saves a [`UiPatch`] to the user's `settings.json`: the patch's fields replace those in the
/// file's `ui` section, and every other key stays, in its order, unknown keys included. The
/// file is written as plain JSON, so its comments are lost. A file that does not parse is left
/// alone and the save fails, so nothing the user wrote is lost. It does not check policy locks:
/// [`crate::Effective::apply_ui_patch`] does, before it calls this.
pub(crate) fn write_user_settings(
    locations: &Locations,
    files: &dyn Files,
    patch: &UiPatch,
) -> Result<(), Error> {
    write_section(locations, files, "ui", patch)
}

/// Saves a [`ToolsPatch`] to the user's `settings.json`, into its `tools` section, as
/// [`write_user_settings`] does for `ui`. It does not check policy locks:
/// [`crate::Effective::apply_tools_patch`] does, before it calls this.
pub(crate) fn write_tools_settings(
    locations: &Locations,
    files: &dyn Files,
    patch: &ToolsPatch,
) -> Result<(), Error> {
    write_section(locations, files, "tools", patch)
}

/// Saves a [`PrivacyPatch`] to the user's `settings.json`, into its `privacy` section, as
/// [`write_user_settings`] does for `ui`. It does not check policy locks:
/// [`crate::Effective::apply_privacy_patch`] does, before it calls this.
pub(crate) fn write_privacy_settings(
    locations: &Locations,
    files: &dyn Files,
    patch: &PrivacyPatch,
) -> Result<(), Error> {
    write_section(locations, files, "privacy", patch)
}

/// Saves the lawyer's `update.mode` to the user's `settings.json`, into its `update` section, as
/// [`write_user_settings`] does for `ui`. It does not check policy locks:
/// [`crate::Effective::apply_update_mode`] does, before it calls this.
pub(crate) fn write_update_mode(
    locations: &Locations,
    files: &dyn Files,
    mode: UpdateChoice,
) -> Result<(), Error> {
    write_section(locations, files, "update", &UpdateFields { mode })
}

/// Saves the fields of `patch` that are set into the section `name` of the user's
/// `settings.json`, the rest of the file as it is (see [`write_user_settings`]).
fn write_section<P: Serialize>(
    locations: &Locations,
    files: &dyn Files,
    name: &str,
    patch: &P,
) -> Result<(), Error> {
    let mut document = read_document(locations, files)?;
    if let (Value::Object(section), Ok(Value::Object(fields))) = (
        section_mut(&mut document, name),
        serde_json_lenient::to_value(patch),
    ) {
        section.extend(fields);
    }
    save_document(locations, files, &document)
}

/// Saves one entry of the section `section` (`agents` or `plans`) in the user's
/// `settings.json`: `entry` replaces the file's entry `id`, or `None` removes it, and every
/// other key stays, as [`write_user_settings`] keeps them. When the file already holds an entry
/// `id` and `keep` says it stays, nothing is written and the result is `Ok(false)`.
pub(crate) fn write_entry(
    locations: &Locations,
    files: &dyn Files,
    (section, id): (&str, &str),
    entry: Option<Value>,
    keep: impl Fn(&serde_json::Value) -> bool,
) -> Result<bool, Error> {
    let mut document = read_document(locations, files)?;
    let Value::Object(entries) = section_mut(&mut document, section) else {
        return Ok(false);
    };
    let held = entries
        .get(id)
        .and_then(|held| serde_json_lenient::from_value::<serde_json::Value>(held.clone()).ok());
    if entries.contains_key(id) && held.as_ref().is_none_or(&keep) {
        return Ok(false);
    }
    match entry {
        Some(entry) => {
            entries.insert(id.to_owned(), entry);
        }
        None => {
            entries.remove(id);
        }
    }
    save_document(locations, files, &document)?;
    Ok(true)
}

/// Saves one entry of `skills.enabled` in the user's `settings.json`: `hash` becomes the entry
/// `id`, or `None` removes it, and every other key stays, as [`write_user_settings`] keeps
/// them. A `skills` or `enabled` that is not an object is replaced by one. It checks neither
/// locks nor the id and hash: [`crate::Effective::set_skill_pack`] does, before it calls this.
pub(crate) fn write_skill_pack(
    locations: &Locations,
    files: &dyn Files,
    id: &str,
    hash: Option<&str>,
) -> Result<(), Error> {
    let mut document = read_document(locations, files)?;
    if let Value::Object(skills) = section_mut(&mut document, "skills")
        && let Value::Object(entries) = section_mut(skills, "enabled")
    {
        match hash {
            Some(hash) => {
                entries.insert(id.to_owned(), Value::String(hash.to_owned()));
            }
            None => {
                entries.remove(id);
            }
        }
    }
    save_document(locations, files, &document)
}

/// The user's `settings.json` as one object: empty when the file is missing; an error, with
/// the file left alone, when it does not parse.
fn read_document(locations: &Locations, files: &dyn Files) -> Result<Map<String, Value>, Error> {
    let path = locations.settings_file();
    match files.read(path).map_err(|source| io(path, source))? {
        None => Ok(Map::new()),
        Some(bytes) => object(path, &bytes),
    }
}

/// The section `name` of `document`, made an empty object when it is missing or not one.
fn section_mut<'a>(document: &'a mut Map<String, Value>, name: &str) -> &'a mut Value {
    document
        .entry(name)
        .and_modify(|section| {
            if !section.is_object() {
                *section = Value::Object(Map::new());
            }
        })
        .or_insert_with(|| Value::Object(Map::new()))
}

/// Writes `document` to the user's `settings.json` as plain JSON, atomically.
fn save_document(
    locations: &Locations,
    files: &dyn Files,
    document: &Map<String, Value>,
) -> Result<(), Error> {
    let path = locations.settings_file();
    let mut bytes =
        serde_json_lenient::to_vec_pretty(document).map_err(|error| io(path, error.into()))?;
    bytes.push(b'\n');
    files
        .write_atomic(path, &bytes)
        .map_err(|source| io(path, source))
}

/// An I/O error on `path`.
fn io(path: &Path, source: std::io::Error) -> Error {
    Error::Io {
        path: path.to_owned(),
        source,
    }
}

/// The file's top-level object, or a parse error with its line and column. A file of only
/// whitespace and comments is an empty object.
fn object(path: &Path, bytes: &[u8]) -> Result<Map<String, Value>, Error> {
    let text = bytes.strip_prefix(json::BOM).unwrap_or(bytes);
    if json::is_blank(text) {
        return Ok(Map::new());
    }
    serde_json_lenient::from_slice(text).map_err(|error| {
        let syntax = json::syntax(&error);
        Error::Parse {
            path: path.to_owned(),
            line: syntax.position.line,
            column: syntax.position.column,
            message: syntax.message,
        }
    })
}
