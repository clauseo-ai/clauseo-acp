//! The user's `settings.json`: reading it, and the one write, a [`UiPatch`].

#[cfg(test)]
mod tests;

use crate::error::Error;
use crate::files::Files;
use crate::json;
use crate::leaf::{self, Leaf};
use crate::locations::Locations;
use crate::ui::UiPatch;
use crate::warning::{Origin, Warning, WarningKind};
use serde_json_lenient::{Map, Value};
use std::path::Path;

/// The user's values, in file order. A missing file holds none. A file that cannot be read or
/// parsed holds none either, with a warning: a broken file never stops the app from starting.
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
        Ok((raws, file_warnings)) => {
            warnings.extend(file_warnings);
            leaf::typed(raws, warnings)
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
    let path = locations.settings_file();
    let io = |source| Error::Io {
        path: path.to_owned(),
        source,
    };
    let mut document = match files.read(path).map_err(io)? {
        None => Map::new(),
        Some(bytes) => object(path, &bytes)?,
    };
    let ui = document
        .entry("ui")
        .and_modify(|ui| {
            if !ui.is_object() {
                *ui = Value::Object(Map::new());
            }
        })
        .or_insert_with(|| Value::Object(Map::new()));
    if let (Value::Object(ui), Ok(Value::Object(fields))) =
        (ui, serde_json_lenient::to_value(patch))
    {
        ui.extend(fields);
    }
    let mut bytes =
        serde_json_lenient::to_vec_pretty(&document).map_err(|error| io(error.into()))?;
    bytes.push(b'\n');
    files.write_atomic(path, &bytes).map_err(io)
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
