//! Every skill in a tree, found as the skills.sh installer finds them (vercel-labs/skills
//! `discoverSkills`, v1.9.0):
//!
//! 1. A `SKILL.md` at the top: that skill alone.
//! 2. Else the top's own folders (one level), then `skills/`, `skills/.curated/`,
//!    `skills/.experimental/`, `skills/.system/`, `.agents/skills/`, `.claude/skills/` and the
//!    folders `skills.sh.json` lists in `paths`, each up to [`CONTAINER_DEPTH`] levels down.
//!    A folder holding `SKILL.md` is a skill and is not looked under, so a shallower skill
//!    shadows deeper ones; a name found before shadows the same name found later.
//! 3. Nothing found: every folder down to [`FALLBACK_DEPTH`].
//!
//! `.git`, `node_modules`, `dist`, `build` and `__pycache__` are never walked; symlinks (and
//! Windows reparse points) are never followed. A skill needs a frontmatter with `name` and
//! `description`; one with `metadata.internal: true` is hidden unless asked for by name. Each
//! skill found is read as `added` reads it and built as a pack in memory, so a skill too big or
//! breaking a pack rule is reported with its refusal, never dropped. Off the host thread.

use std::collections::BTreeSet;
use std::io::Read as _;
use std::path::{Path, PathBuf};

use clauseo_protocol::Date;
use serde_json::Value;

use super::super::read::{PackFile, is_reparse};
use super::super::render::front_name;
use super::super::{FILE_MAX, Refusal, added, scripts};
use super::SourceError;
use crate::skills::{SKILL_MD, frontmatter};

/// How far below a known skill folder (`skills/`, ...) a skill is looked for.
pub const CONTAINER_DEPTH: usize = 3;

/// How far below the top the fallback search looks.
pub const FALLBACK_DEPTH: usize = 5;

/// The folders never walked.
const SKIP_DIRS: [&str; 5] = [".git", "node_modules", "dist", "build", "__pycache__"];

/// The known skill folders, below the top.
const CONTAINERS: [&str; 6] = [
    "skills",
    "skills/.curated",
    "skills/.experimental",
    "skills/.system",
    ".agents/skills",
    ".claude/skills",
];

/// The repository's own list of skill folders.
const CONFIG_FILE: &str = "skills.sh.json";

/// The most folders one discovery visits, so a large folder picked by mistake ends quickly.
const VISITS_MAX: usize = 5000;

/// One skill found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// Its folder, relative to the tree's top, `/`-separated; empty for the top itself.
    pub folder: String,
    /// The frontmatter's `name`.
    pub name: String,
    /// The pack id it would be added as (`added::slug`), when the name gives one.
    pub slug: Option<String>,
    /// The frontmatter's `description`, on one line.
    pub description: String,
    /// Whether it holds a file an agent could run: anything but Markdown, text, data and
    /// images, or a file with an exec bit or executable magic (`scripts::is_script`).
    pub has_scripts: bool,
    /// Its bytes, all files together (as far as counted, when too big).
    pub bytes: u64,
    /// Its files (as far as counted, when too big).
    pub files: usize,
    /// Why it cannot be added as it is: `TooBig` past `added`'s caps, else the pack rule.
    pub problem: Option<Refusal>,
}

/// The skills in the tree at `top`; with `asked`, only those whose `name` or folder name is it
/// (ASCII case ignored, spaces and `_` read as `-`), hidden ones included. `NotFound` when none.
pub fn discover(top: &Path, asked: Option<&str>) -> Result<Vec<Found>, SourceError> {
    let mut walk = Walk {
        top,
        asked,
        visits: 0,
        names: BTreeSet::new(),
        found: Vec::new(),
    };
    if walk.try_add(top) && !walk.found.is_empty() {
        return Ok(walk.finish());
    }
    walk.found.clear();
    walk.walk(top, 1, 1);
    let mut containers: Vec<PathBuf> = CONTAINERS.iter().map(|dir| top.join(dir)).collect();
    containers.extend(config_paths(top).into_iter().map(|dir| top.join(dir)));
    for dir in containers {
        walk.walk(&dir, CONTAINER_DEPTH, 1);
    }
    if walk.found.is_empty() {
        walk.fallback(top, 0);
    }
    let found = walk.finish();
    if found.is_empty() {
        return Err(SourceError::NotFound);
    }
    Ok(found)
}

/// `found` narrowed to the skill named `asked`, as [`discover`] narrows.
pub fn narrow(found: Vec<Found>, asked: &str) -> Vec<Found> {
    found
        .into_iter()
        .filter(|skill| matches(asked, &skill.name, &skill.folder))
        .collect()
}

/// Whether `asked` names the skill `name` in `folder`.
fn matches(asked: &str, name: &str, folder: &str) -> bool {
    let folder_name = folder.rsplit('/').next().unwrap_or_default();
    let norm = |text: &str| text.to_ascii_lowercase().replace([' ', '_'], "-");
    [name, folder_name]
        .iter()
        .any(|text| !text.is_empty() && norm(text) == norm(asked))
}

/// The discovery's state.
struct Walk<'a> {
    top: &'a Path,
    asked: Option<&'a str>,
    visits: usize,
    names: BTreeSet<String>,
    found: Vec<Found>,
}

impl Walk<'_> {
    /// Looks under `dir`, `depth` levels below where the walk started, at most `max` levels.
    fn walk(&mut self, dir: &Path, max: usize, depth: usize) {
        for (child, name) in self.folders(dir) {
            if self.try_add(&child) || depth >= max || SKIP_DIRS.contains(&name.as_str()) {
                continue;
            }
            self.walk(&child, max, depth + 1);
        }
    }

    /// Every folder down to [`FALLBACK_DEPTH`] holding `SKILL.md`.
    fn fallback(&mut self, dir: &Path, depth: usize) {
        if depth > FALLBACK_DEPTH {
            return;
        }
        self.try_add(dir);
        for (child, name) in self.folders(dir) {
            if !SKIP_DIRS.contains(&name.as_str()) {
                self.fallback(&child, depth + 1);
            }
        }
    }

    /// The real folders in `dir` (no links), sorted by name; none past [`VISITS_MAX`].
    fn folders(&mut self, dir: &Path) -> Vec<(PathBuf, String)> {
        self.visits += 1;
        if self.visits > VISITS_MAX || !is_real(dir, true) {
            return Vec::new();
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return Vec::new();
        };
        let mut out: Vec<(PathBuf, String)> = entries
            .flatten()
            .filter_map(|entry| Some((entry.path(), entry.file_name().into_string().ok()?)))
            .filter(|(path, _)| is_real(path, true))
            .collect();
        out.sort_by(|a, b| a.1.cmp(&b.1));
        out
    }

    /// Whether `dir` holds a `SKILL.md`; when it does and its frontmatter is usable, the skill
    /// is added unless its name was found before, or it is hidden or not the one asked for.
    fn try_add(&mut self, dir: &Path) -> bool {
        let skill_md = dir.join(SKILL_MD);
        if !is_real(&skill_md, false) {
            return false;
        }
        let Some(head) = head_of(&skill_md) else {
            return true;
        };
        let folder = self.relative(dir);
        let asked_ok = self
            .asked
            .is_none_or(|asked| matches(asked, &head.name, &folder));
        if (head.internal && self.asked.is_none()) || !asked_ok {
            return true;
        }
        if self.names.insert(head.name.clone()) {
            self.found.push(measure(dir, folder, head));
        }
        true
    }

    /// `dir` relative to the top, `/`-separated.
    fn relative(&self, dir: &Path) -> String {
        let rel = dir.strip_prefix(self.top).unwrap_or(dir);
        let parts: Vec<String> = rel
            .components()
            .map(|part| part.as_os_str().to_string_lossy().into_owned())
            .collect();
        parts.join("/")
    }

    fn finish(self) -> Vec<Found> {
        self.found
    }
}

/// Whether `path` is a folder (`dir`) or a file, and no link or reparse point.
fn is_real(path: &Path, dir: bool) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| {
        !meta.file_type().is_symlink()
            && !is_reparse(&meta)
            && if dir { meta.is_dir() } else { meta.is_file() }
    })
}

/// What a `SKILL.md`'s frontmatter says.
struct Head {
    name: String,
    description: String,
    internal: bool,
}

/// The frontmatter of the `SKILL.md` at `path`: `None` without a `name` and a `description`.
fn head_of(path: &Path) -> Option<Head> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(FILE_MAX)
        .read_to_end(&mut bytes)
        .ok()?;
    let text = String::from_utf8(bytes).ok()?;
    let text = text
        .strip_prefix('\u{feff}')
        .unwrap_or(&text)
        .replace("\r\n", "\n");
    let (description, body) = frontmatter::lenient_description(&text).ok()?;
    let head = text.get(..text.len() - body.len())?;
    let name = front_name(head)?.trim().to_owned();
    (!name.is_empty()).then(|| Head {
        name,
        description: description.split_whitespace().collect::<Vec<_>>().join(" "),
        internal: is_internal(head),
    })
}

/// Whether the frontmatter `head` says `metadata.internal` is `true` (or `"true"`, `1`).
fn is_internal(head: &str) -> bool {
    let mut in_metadata = false;
    for line in head.lines() {
        if !line.starts_with([' ', '\t']) {
            in_metadata = line.trim_end() == "metadata:";
            continue;
        }
        let Some((key, value)) = line.trim().split_once(':') else {
            continue;
        };
        let value = value.trim().trim_matches(['"', '\'']);
        if in_metadata && key.trim() == "internal" && matches!(value, "true" | "1") {
            return true;
        }
    }
    false
}

/// The skill in `dir`: counted, then read and built as `added` would add it.
fn measure(dir: &Path, folder: String, head: Head) -> Found {
    let mut found = Found {
        folder,
        slug: added::slug(&head.name),
        name: head.name,
        description: head.description,
        has_scripts: false,
        bytes: 0,
        files: 0,
        problem: None,
    };
    count(dir, &mut found);
    if found.files > added::ADD_FILES_MAX || found.bytes > added::ADD_BYTES_MAX {
        found.problem = Some(Refusal::TooBig);
        return found;
    }
    let checked = added::read_source(dir).and_then(|source| {
        let scripts = source.files.iter().any(|(path, bytes)| {
            scripts::is_script(&PackFile {
                path: path.clone(),
                bytes: bytes.clone(),
                exec: false,
            })
        });
        let day = Date::new(2026, 1, 1).map_err(|_bad| added::AddError::Invalid(Refusal::Io))?;
        added::build(&source, &source.slug, day, true)?;
        Ok(scripts)
    });
    match checked {
        Ok(scripts) => found.has_scripts |= scripts,
        Err(added::AddError::Invalid(refusal)) => found.problem = Some(refusal),
        Err(added::AddError::NoSkillMd) => found.problem = Some(Refusal::Skill),
    }
    found
}

/// Counts the files and bytes `added` would copy from `dir`, and notes a file with an exec bit
/// or a name no plain file has; stops once past `added`'s caps.
fn count(dir: &Path, found: &mut Found) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if found.files > added::ADD_FILES_MAX || found.bytes > added::ADD_BYTES_MAX {
            return;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if added::is_left_out(name) {
            continue;
        }
        let Ok(meta) = std::fs::symlink_metadata(entry.path()) else {
            continue;
        };
        if meta.file_type().is_symlink() || is_reparse(&meta) {
            continue;
        }
        if meta.is_dir() {
            count(&entry.path(), found);
        } else if meta.is_file() {
            found.files += 1;
            found.bytes = found.bytes.saturating_add(meta.len());
            let plain = PackFile {
                path: name.to_owned(),
                bytes: Vec::new(),
                exec: has_exec_bit(&meta),
            };
            found.has_scripts |= scripts::is_script(&plain);
        }
    }
}

/// Whether any Unix exec bit is set (never on Windows).
#[cfg(unix)]
fn has_exec_bit(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt as _;
    meta.permissions().mode() & 0o111 != 0
}

/// No exec bits off Unix.
#[cfg(not(unix))]
fn has_exec_bit(_meta: &std::fs::Metadata) -> bool {
    false
}

/// The folders `skills.sh.json` at `top` lists in `paths`: each starts with `./`, holds no
/// `\`, `..`, empty or `.` part, and is not the top. An unreadable file lists none.
fn config_paths(top: &Path) -> Vec<String> {
    let path = top.join(CONFIG_FILE);
    if !is_real(&path, false) {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    let read = std::fs::File::open(&path).and_then(|file| {
        file.take(super::super::MANIFEST_MAX)
            .read_to_end(&mut bytes)
    });
    let parsed = read.map(|_read| serde_json::from_slice::<Value>(&bytes).ok());
    let Ok(Some(Value::Object(config))) = parsed else {
        return Vec::new();
    };
    let Some(Value::Array(paths)) = config.get("paths").cloned() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for entry in paths.iter().filter_map(Value::as_str) {
        let Some(rest) = entry.strip_prefix("./") else {
            continue;
        };
        let parts: Vec<&str> = rest.split('/').filter(|part| !part.is_empty()).collect();
        let plain = !entry.contains('\\')
            && !parts.is_empty()
            && parts.iter().all(|part| *part != "." && *part != "..");
        let joined = parts.join("/");
        if plain && !out.contains(&joined) {
            out.push(joined);
        }
    }
    out
}
