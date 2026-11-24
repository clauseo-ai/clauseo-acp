//! `clauseo-pack.json`, read strictly: every field is required, typed by hand, and an unknown
//! key (at the top or in a skill) refuses the pack.
//!
//! ```json
//! {"format": 1, "id": "nda-kit", "name": "NDA review kit", "version": "1.2.0",
//!  "author": "Example Chambers", "source_url": "https://example.org/nda-kit",
//!  "licence": "CC-BY-4.0", "last_reviewed": "2026-10-20", "instruction_only": true,
//!  "skills": [{"name": "nda-review"}]}
//! ```

use std::collections::BTreeSet;

use clauseo_protocol::Date;
use serde_json::{Map, Value};

use super::source::FromKind;
use super::{SEPARATOR, SKILLS_MAX, is_pack_id, is_pack_skill_name};

/// The `format` this Clauseo reads.
pub const FORMAT: u64 = 1;

/// The most characters a display field (`name`, `version`, `author`, `licence`) has.
pub const DISPLAY_MAX: usize = 200;

/// The most bytes `source_url` has.
pub const URL_MAX: usize = 2048;

/// The most bytes a rendered skill name has (`skills::NAME_MAX`).
const RENDERED_MAX: usize = 64;

/// Every key the manifest has.
const KEYS: [&str; 12] = [
    "format",
    "id",
    "name",
    "version",
    "author",
    "source_url",
    "licence",
    "last_reviewed",
    "instruction_only",
    "skills",
    "added_here",
    "added_from",
];

/// The keys a pack added on this computer (`added_here`) leaves out: it has no version, author
/// or licence of its own. Its `source_url` is where it was downloaded from, when it was.
const NOT_ADDED: [&str; 3] = ["version", "author", "licence"];

/// A pack's manifest, checked. The display fields never go into an agent's instructions or a
/// log line; only `id` and the skill names do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackManifest {
    /// The pack's id.
    pub id: String,
    /// Its name, for display.
    pub name: String,
    /// Its version, for display.
    pub version: String,
    /// Its author, for display.
    pub author: String,
    /// Where it came from: an `https` URL, never fetched for a pack; for a skill added here,
    /// where the lawyer downloaded it from (empty for one from this computer).
    pub source_url: String,
    /// Its licence, for display.
    pub licence: String,
    /// When its author last reviewed it.
    pub last_reviewed: Date,
    /// Whether it promises to hold no script.
    pub instruction_only: bool,
    /// Its skills' names, in the order listed.
    pub skills: Vec<String>,
    /// Whether the lawyer added it on this computer (`added_here: true`, written by
    /// `added::build`); then `version`, `author` and `licence` are empty.
    pub added_here: bool,
    /// How a pack added here reached this computer (`added_from`; a folder when the key is
    /// left out, as in every pack added before it); `None` for any other pack.
    pub added_from: Option<FromKind>,
}

/// The manifest in `bytes`, or why it is refused (for the tests; the pack's refusal is
/// `Refusal::Manifest` or `Refusal::BadId`).
pub fn parse(bytes: &[u8]) -> Result<PackManifest, ManifestError> {
    use ManifestError::{BadField, BadId, NotJson, UnknownKey};
    let Ok(Value::Object(top)) = serde_json::from_slice::<Value>(bytes) else {
        return Err(NotJson);
    };
    if let Some(key) = top.keys().find(|key| !KEYS.contains(&key.as_str())) {
        return Err(UnknownKey(key.clone()));
    }
    if top.get("format").and_then(Value::as_u64) != Some(FORMAT) {
        return Err(BadField("format"));
    }
    let id = string(&top, "id")?;
    if !is_pack_id(&id) {
        return Err(BadId);
    }
    let added_here = match top.get("added_here") {
        None => false,
        Some(Value::Bool(true)) => true,
        Some(_) => return Err(BadField("added_here")),
    };
    if added_here {
        if let Some(key) = NOT_ADDED.iter().find(|key| top.contains_key(**key)) {
            return Err(BadField(key));
        }
        let added_from = match top.get("added_from") {
            None => FromKind::Folder,
            Some(value) => value
                .as_str()
                .and_then(FromKind::parse)
                .ok_or(BadField("added_from"))?,
        };
        let source_url = match top.get("source_url") {
            None => String::new(),
            Some(_) => checked_url(string(&top, "source_url")?)?,
        };
        return Ok(PackManifest {
            name: display(&top, "name")?,
            version: String::new(),
            author: String::new(),
            licence: String::new(),
            source_url,
            last_reviewed: last_reviewed(&top)?,
            instruction_only: instruction_only(&top)?,
            skills: skills(&top, &id)?,
            id,
            added_here,
            added_from: Some(added_from),
        });
    }
    if top.contains_key("added_from") {
        return Err(BadField("added_from"));
    }
    let source_url = checked_url(string(&top, "source_url")?)?;
    let last_reviewed = last_reviewed(&top)?;
    let instruction_only = instruction_only(&top)?;
    let skills = skills(&top, &id)?;
    Ok(PackManifest {
        name: display(&top, "name")?,
        version: display(&top, "version")?,
        author: display(&top, "author")?,
        licence: display(&top, "licence")?,
        id,
        source_url,
        last_reviewed,
        instruction_only,
        skills,
        added_here,
        added_from: None,
    })
}

/// `source_url` when it is an `https` URL of at most [`URL_MAX`] bytes with no space or
/// control character.
fn checked_url(source_url: String) -> Result<String, ManifestError> {
    let url_ok = source_url.len() <= URL_MAX
        && source_url.strip_prefix("https://").is_some_and(|rest| {
            !rest.is_empty() && !rest.chars().any(|c| c.is_whitespace() || c.is_control())
        });
    if url_ok {
        Ok(source_url)
    } else {
        Err(ManifestError::BadField("source_url"))
    }
}

/// `last_reviewed`, a date.
fn last_reviewed(top: &Map<String, Value>) -> Result<Date, ManifestError> {
    string(top, "last_reviewed")?
        .parse::<Date>()
        .ok()
        .ok_or(ManifestError::BadField("last_reviewed"))
}

/// `instruction_only`, a boolean.
fn instruction_only(top: &Map<String, Value>) -> Result<bool, ManifestError> {
    top.get("instruction_only")
        .and_then(Value::as_bool)
        .ok_or(ManifestError::BadField("instruction_only"))
}

/// Why a manifest is refused.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ManifestError {
    /// Not a JSON object.
    NotJson,
    /// A key the format does not have.
    UnknownKey(String),
    /// A field missing or of the wrong form.
    BadField(&'static str),
    /// The id is not a pack id, or is reserved.
    BadId,
}

/// The string field `key`.
fn string(top: &Map<String, Value>, key: &'static str) -> Result<String, ManifestError> {
    top.get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(ManifestError::BadField(key))
}

/// The display field `key`: 1 to [`DISPLAY_MAX`] characters, none a control character.
fn display(top: &Map<String, Value>, key: &'static str) -> Result<String, ManifestError> {
    let text = string(top, key)?;
    let count = text.chars().count();
    if count == 0 || count > DISPLAY_MAX || text.chars().any(char::is_control) {
        return Err(ManifestError::BadField(key));
    }
    Ok(text)
}

/// `skills`: 1 to [`SKILLS_MAX`] objects holding `name` alone, each a pack skill name, none
/// twice, and `<id>--<name>` within 64 bytes.
fn skills(top: &Map<String, Value>, id: &str) -> Result<Vec<String>, ManifestError> {
    let bad = ManifestError::BadField("skills");
    let list = top
        .get("skills")
        .and_then(Value::as_array)
        .ok_or(bad.clone())?;
    if list.is_empty() || list.len() > SKILLS_MAX {
        return Err(bad);
    }
    let mut names = Vec::with_capacity(list.len());
    let mut seen = BTreeSet::new();
    for item in list {
        let object = item.as_object().ok_or(bad.clone())?;
        if let Some(key) = object.keys().find(|key| key.as_str() != "name") {
            return Err(ManifestError::UnknownKey(format!("skills.{key}")));
        }
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .ok_or(bad.clone())?;
        let fits = id.len() + SEPARATOR.len() + name.len() <= RENDERED_MAX;
        if !is_pack_skill_name(name) || !fits || !seen.insert(name) {
            return Err(bad);
        }
        names.push(name.to_owned());
    }
    Ok(names)
}
