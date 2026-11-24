//! A skill added on this computer (`AddSkill`, decision record 0040): a folder
//! holding `SKILL.md`, or a `SKILL.md` alone, made into a skill pack of one skill.
//!
//! - [`read_source`] reads it into memory: symlinks (and on Windows every reparse point), hidden
//!   names (`.DS_Store`, `.git`) and the names skills.sh leaves out ([`is_left_out`]) are
//!   skipped, never followed, and so is anything that is
//!   neither a file nor a folder; the caps ([`ADD_BYTES_MAX`], [`ADD_FILES_MAX`],
//!   [`FOLDERS_MAX`], [`FILE_MAX`] a file) stop the read early. Its `SKILL.md`'s frontmatter
//!   gives the display name, the id (the name slugged to the pack id rules, [`slug`]) and the
//!   description, and is cut to `name` and `description` alone, so a vendor's `SKILL.md` with
//!   other keys is taken.
//! - [`build`] makes the pack's files in memory with its own `clauseo-pack.json`, marked
//!   `added_here` (no `version`, `author` or `licence`; `last_reviewed` the day it was added;
//!   [`build_from`] records where it came from in `added_from` and `source_url`), and checks
//!   them as a launch checks a pack (`render::check_and_render`), so what is written is a pack
//!   that loads.
//! - [`free_id`] picks the folder: the slug, or when a pack not added here holds it, the slug
//!   with `-2`, `-3`, … So a skill added again replaces the one added before it.
//! - [`install`] writes the pack into a staging folder, then renames it over the old one.
//! - [`is_added_here`] reads a pack folder's marker; [`remove`] deletes a pack added here.
//!
//! The marker is the lawyer's own: it admits the pack whatever the lawyer's `skills.extra`
//! says, never past a firm's policy (the host checks that).

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use clauseo_protocol::Date;
use serde_json::{Value, json};

use super::read::{PackFile, PackRead, is_reparse};
use super::render::{DESCRIPTION_MAX, Pack, front_name};
use super::source::{FromKind, Origin};
use super::{
    DEPTH_MAX, FILE_MAX, FOLDERS_MAX, MANIFEST_MAX, PACK_MANIFEST, PATH_MAX, Refusal, SKILLS_DIR,
    is_pack_id, is_pack_skill_name, render,
};
use crate::skills::builtin::is_unlisted_char;
use crate::skills::{SKILL_MD, frontmatter, is_safe_path};
use crate::write;

/// The most bytes a skill added on this computer has, all its files together.
pub const ADD_BYTES_MAX: u64 = 5_000_000;

/// The most files a skill added on this computer has.
pub const ADD_FILES_MAX: usize = 200;

/// The manifest key that marks a pack added on this computer.
pub const MARKER: &str = "added_here";

/// The manifest key that says how a pack added on this computer reached it
/// ([`FromKind::as_str`]); none for a folder.
pub const FROM_KEY: &str = "added_from";

/// The most bytes of a display name (`name` in the manifest) is the manifest's own cap.
const NAME_MAX: usize = super::manifest::DISPLAY_MAX;

/// The most bytes of a listed name: what `add_skills` takes as a picked name.
const NAME_BYTES_MAX: usize = 200;

/// The longest id the slug gives, leaving room for a `-<n>` when the slug is taken.
const SLUG_MAX: usize = 27;

/// The names a skill's copy leaves out, as the skills.sh installer does, besides every hidden
/// name (`.git`, `.DS_Store`).
const LEFT_OUT: [&str; 3] = ["metadata.json", "__pycache__", "__pypackages__"];

/// Whether a file or folder named `name` is left out of a skill's copy: hidden, or on
/// [`LEFT_OUT`].
pub fn is_left_out(name: &str) -> bool {
    name.starts_with('.') || LEFT_OUT.contains(&name)
}

/// Where an added skill came from, recorded in its manifest: `added_from` (left out for a
/// folder, so every pack added before it reads as one) and `source_url` (`https` only).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// How it reached this computer.
    pub from: FromKind,
    /// The skills.sh page, the GitHub link or the `.zip` link.
    pub source_url: Option<String>,
}

impl Record {
    /// A folder or `SKILL.md` on this computer: nothing more recorded.
    pub fn folder() -> Self {
        Self {
            from: FromKind::Folder,
            source_url: None,
        }
    }

    /// What `origin` says.
    pub fn of(origin: &Origin) -> Self {
        Self {
            from: origin.kind,
            source_url: origin.source_url.clone(),
        }
    }
}

/// Why a skill cannot be added.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddError {
    /// No `SKILL.md` at the path: neither a folder holding one nor a `SKILL.md` file.
    NoSkillMd,
    /// It breaks a pack rule: `Io` for a read or write that failed, `TooBig` past
    /// [`ADD_BYTES_MAX`], [`ADD_FILES_MAX`], [`FOLDERS_MAX`] or a file past [`FILE_MAX`].
    Invalid(Refusal),
}

/// A skill read from this computer, `SKILL.md` already cut to `name` and `description`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    /// The display name, from the frontmatter's `name`.
    pub name: String,
    /// The id the name slugs to, also the skill's name inside the pack.
    pub slug: String,
    /// Its files, paths relative to the skill's folder, `SKILL.md` among them.
    pub files: Vec<(String, Vec<u8>)>,
}

/// The pack's id from a skill's `name`: lowercase ASCII letters and digits kept, every run of
/// anything else one `-`, no `-` at either end, at most 27 bytes. `None` when that is no pack
/// id (empty, one byte, reserved, a device name).
pub fn slug(name: &str) -> Option<String> {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.truncate(SLUG_MAX);
    let out = out.trim_end_matches('-').to_owned();
    (is_pack_id(&out) && is_pack_skill_name(&out)).then_some(out)
}

/// Reads the skill at `path`: a folder holding `SKILL.md` (any case), or a `SKILL.md` file.
/// Off the host thread.
pub fn read_source(path: &Path) -> Result<Source, AddError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Err(AddError::NoSkillMd);
        }
        Err(_) => return Err(AddError::Invalid(Refusal::Io)),
    };
    let mut walk = Walk::default();
    if meta.file_type().is_symlink() || is_reparse(&meta) {
        return Err(AddError::NoSkillMd);
    }
    if meta.is_file() {
        let named = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case(SKILL_MD));
        if !named {
            return Err(AddError::NoSkillMd);
        }
        walk.file(path, SKILL_MD.to_owned(), meta.len())?;
    } else if meta.is_dir() {
        walk.folder(path, "")?;
    } else {
        return Err(AddError::NoSkillMd);
    }
    walk.into_source()
}

/// The reader of [`read_source`].
#[derive(Default)]
struct Walk {
    files: Vec<(String, Vec<u8>)>,
    folders: usize,
    total: u64,
}

impl Walk {
    /// Reads the folder `dir`, at `prefix` inside the skill.
    fn folder(&mut self, dir: &Path, prefix: &str) -> Result<(), AddError> {
        let entries = std::fs::read_dir(dir).map_err(failed(Refusal::Io))?;
        let mut names = Vec::new();
        for entry in entries {
            let entry = entry.map_err(failed(Refusal::Io))?;
            names.push(entry.file_name());
        }
        names.sort();
        for name in names {
            let Some(text) = name.to_str() else {
                return Err(AddError::Invalid(Refusal::BadPath));
            };
            if is_left_out(text) {
                continue;
            }
            let full = dir.join(&name);
            let meta = std::fs::symlink_metadata(&full).map_err(failed(Refusal::Io))?;
            if meta.file_type().is_symlink() || is_reparse(&meta) {
                continue;
            }
            let rel = if prefix.is_empty() {
                // The top `SKILL.md` in any case is `SKILL.md`.
                if meta.is_file() && text.eq_ignore_ascii_case(SKILL_MD) {
                    SKILL_MD.to_owned()
                } else {
                    text.to_owned()
                }
            } else {
                format!("{prefix}/{text}")
            };
            if meta.is_dir() {
                self.folders += 1;
                if self.folders > FOLDERS_MAX {
                    return Err(AddError::Invalid(Refusal::TooBig));
                }
                if rel.matches('/').count() >= DEPTH_MAX {
                    return Err(AddError::Invalid(Refusal::BadPath));
                }
                self.folder(&full, &rel)?;
            } else if meta.is_file() {
                self.file(&full, rel, meta.len())?;
            }
        }
        Ok(())
    }

    /// Reads the file `path` as `rel`, within the caps.
    fn file(&mut self, path: &Path, rel: String, len: u64) -> Result<(), AddError> {
        if self.files.len() >= ADD_FILES_MAX || len > FILE_MAX || self.total + len > ADD_BYTES_MAX {
            return Err(AddError::Invalid(Refusal::TooBig));
        }
        let file = std::fs::File::open(path).map_err(failed(Refusal::Io))?;
        let mut bytes = Vec::new();
        file.take(FILE_MAX + 1)
            .read_to_end(&mut bytes)
            .map_err(failed(Refusal::Io))?;
        let read = bytes.len() as u64;
        if read > FILE_MAX || self.total + read > ADD_BYTES_MAX {
            return Err(AddError::Invalid(Refusal::TooBig));
        }
        self.total += read;
        self.files.push((rel, bytes));
        Ok(())
    }

    /// The source, from its `SKILL.md`'s frontmatter.
    fn into_source(mut self) -> Result<Source, AddError> {
        let skill_md = self
            .files
            .iter_mut()
            .find(|(path, _)| path == SKILL_MD)
            .ok_or(AddError::NoSkillMd)?;
        let invalid = AddError::Invalid(Refusal::Skill);
        let text = std::str::from_utf8(&skill_md.1).map_err(failed(Refusal::Skill))?;
        let text = text
            .strip_prefix('\u{feff}')
            .unwrap_or(text)
            .replace("\r\n", "\n");
        let (description, body) =
            frontmatter::lenient_description(&text).map_err(failed(Refusal::Skill))?;
        let head = text.get(..text.len() - body.len()).ok_or(invalid)?;
        let name = front_name(head)
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .ok_or(invalid)?;
        let slug = slug(name).ok_or(AddError::Invalid(Refusal::BadId))?;
        let description = one_line(&description);
        if description.is_empty() || description.chars().count() > DESCRIPTION_MAX {
            return Err(invalid);
        }
        let display = display_name(name);
        let display = if display.is_empty() {
            slug.clone()
        } else {
            display
        };
        skill_md.1 = format!("{}{body}", frontmatter::render(&slug, &description)).into_bytes();
        Ok(Source {
            name: display,
            slug,
            files: self.files,
        })
    }
}

/// A `map_err` closure that turns any error into `refusal`, as `packs::refuse` does.
fn failed<E>(refusal: Refusal) -> impl FnOnce(E) -> AddError {
    move |_| AddError::Invalid(refusal)
}

/// The name a skill named `name` is listed by, as the preview shows it and the pack keeps it:
/// control and unlisted characters (bidi overrides, zero-width marks) gone, trimmed, at most
/// [`NAME_MAX`] characters and [`NAME_BYTES_MAX`] bytes, so a name shown can be picked. May be
/// empty (the slug stands in then).
pub fn display_name(name: &str) -> String {
    let mut out = String::new();
    let kept = name
        .chars()
        .filter(|c| !c.is_control() && !is_unlisted_char(*c))
        .take(NAME_MAX);
    for c in kept {
        if out.len() + c.len_utf8() > NAME_BYTES_MAX {
            break;
        }
        out.push(c);
    }
    out.trim().to_owned()
}

/// `text` on one line: whitespace of any kind one space, other unlisted characters gone, as a
/// pack's render does.
pub fn one_line(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !is_unlisted_char(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// The pack `id` made from `source`, added on `today`, checked as a launch checks a pack: its
/// files (pack-relative paths) and the pack as read. Pure.
pub fn build(
    source: &Source,
    id: &str,
    today: Date,
    allow_scripts: bool,
) -> Result<(Vec<PackFile>, Pack), AddError> {
    build_from(source, id, today, allow_scripts, &Record::folder())
}

/// [`build`], recording `record` in the manifest.
pub fn build_from(
    source: &Source,
    id: &str,
    today: Date,
    allow_scripts: bool,
    record: &Record,
) -> Result<(Vec<PackFile>, Pack), AddError> {
    let invalid = AddError::Invalid;
    if !is_pack_id(id) {
        return Err(invalid(Refusal::BadId));
    }
    let skill_dir = format!("{SKILLS_DIR}/{}", source.slug);
    let mut files: Vec<PackFile> = source
        .files
        .iter()
        .map(|(path, bytes)| PackFile {
            path: format!("{skill_dir}/{path}"),
            bytes: bytes.clone(),
            exec: false,
        })
        .collect();
    let instruction_only = !files.iter().any(super::scripts::is_script);
    let mut manifest = json!({
        "format": super::manifest::FORMAT,
        "id": id,
        "name": source.name,
        "last_reviewed": today.to_string(),
        "instruction_only": instruction_only,
        "skills": [{"name": source.slug}],
        MARKER: true,
    });
    if let Some(top) = manifest.as_object_mut() {
        if record.from != FromKind::Folder {
            top.insert(FROM_KEY.to_owned(), json!(record.from.as_str()));
        }
        if let Some(url) = &record.source_url {
            top.insert("source_url".to_owned(), json!(url));
        }
    }
    let manifest = serde_json::to_vec_pretty(&manifest).map_err(failed(Refusal::Manifest))?;
    files.push(PackFile {
        path: PACK_MANIFEST.to_owned(),
        bytes: manifest,
        exec: false,
    });
    let mut folders = BTreeSet::from([SKILLS_DIR.to_owned(), skill_dir]);
    let mut seen = BTreeSet::new();
    for file in &files {
        let path = &file.path;
        if !is_safe_path(path)
            || path.len() > PATH_MAX
            || path.matches('/').count() > DEPTH_MAX
            || !seen.insert(path.to_ascii_lowercase())
        {
            return Err(invalid(Refusal::BadPath));
        }
        let mut at = path.as_str();
        while let Some((parent, _)) = at.rsplit_once('/') {
            folders.insert(parent.to_owned());
            at = parent;
        }
    }
    let read = PackRead {
        files: files.clone(),
        folders: folders.into_iter().collect(),
        open_peak: 0,
    };
    let pack = render::check_and_render(id, read, allow_scripts).map_err(invalid)?;
    Ok((files, pack))
}

/// The folder name for a skill whose slug is `slug` under `packs_root`: the slug when no pack
/// holds it or a pack added here does, else the first of `<slug>-2` to `<slug>-99` that is
/// free or added here. Off the host thread.
pub fn free_id(packs_root: &Path, slug: &str) -> Result<String, AddError> {
    std::iter::once(slug.to_owned())
        .chain((2..100).map(|n| format!("{slug}-{n}")))
        .filter(|id| is_pack_id(id))
        .find(|id| {
            let folder = packs_root.join(id);
            std::fs::symlink_metadata(&folder).is_err() || is_added_here(&folder)
        })
        .ok_or(AddError::Invalid(Refusal::BadId))
}

/// Writes `files` as the pack `id` under `packs_root`, through `staging` (a folder beside it on
/// the same disk): written there whole, then renamed over the pack added before it, which is
/// then deleted. A failure leaves the old pack in place. Off the host thread.
pub fn install(
    packs_root: &Path,
    staging: &Path,
    id: &str,
    files: &[PackFile],
) -> Result<(), AddError> {
    let io = failed(Refusal::Io);
    let fresh = staging.join(id);
    let old = staging.join(format!("{id}.old"));
    write::remove_dir_all(&fresh);
    write::remove_dir_all(&old);
    write::create_dir_all(packs_root).map_err(failed(Refusal::Io))?;
    for file in files {
        write::write_new(&fresh.join(&file.path), &file.bytes).map_err(failed(Refusal::Io))?;
    }
    let target = packs_root.join(id);
    let replacing = std::fs::symlink_metadata(&target).is_ok();
    if replacing {
        write::rename(&target, &old).map_err(failed(Refusal::Io))?;
    }
    if let Err(error) = write::rename(&fresh, &target) {
        if replacing {
            let _back = write::rename(&old, &target);
        }
        write::remove_dir_all(&fresh);
        return Err(io(error));
    }
    write::remove_dir_all(&old);
    Ok(())
}

/// Whether the folder `folder` is a pack added on this computer: a folder (no link) whose
/// `clauseo-pack.json` (a file, no link, within its cap) is a JSON object with `added_here`
/// `true`. Off the host thread.
pub fn is_added_here(folder: &Path) -> bool {
    let plain = |path: &Path| {
        std::fs::symlink_metadata(path)
            .ok()
            .filter(|meta| !meta.file_type().is_symlink() && !is_reparse(meta))
    };
    if !plain(folder).is_some_and(|meta| meta.is_dir()) {
        return false;
    }
    let manifest = folder.join(PACK_MANIFEST);
    if !plain(&manifest).is_some_and(|meta| meta.is_file() && meta.len() <= MANIFEST_MAX) {
        return false;
    }
    let Ok(bytes) = std::fs::read(&manifest) else {
        return false;
    };
    serde_json::from_slice::<Value>(&bytes)
        .ok()
        .and_then(|value| value.get(MARKER).and_then(Value::as_bool))
        .unwrap_or(false)
}

/// Deletes the pack `id` under `packs_root`, when it was added here: `Ok(false)` (nothing
/// deleted) for a pack not added here or none. Off the host thread.
pub fn remove(packs_root: &Path, id: &str) -> std::io::Result<bool> {
    if !is_pack_id(id) {
        return Ok(false);
    }
    let folder: PathBuf = packs_root.join(id);
    if !is_added_here(&folder) {
        return Ok(false);
    }
    write::remove_dir_all(&folder);
    if std::fs::symlink_metadata(&folder).is_ok() {
        return Err(std::io::Error::other("the pack's folder stays"));
    }
    Ok(true)
}

/// The UTC date of `unix_millis`, for the manifest's `last_reviewed`.
pub fn utc_date(unix_millis: i64) -> Option<Date> {
    let days = unix_millis.div_euclid(86_400_000);
    // Howard Hinnant's civil_from_days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    Date::new(
        u16::try_from(year).ok()?,
        u8::try_from(month).ok()?,
        u8::try_from(day).ok()?,
    )
    .ok()
}
