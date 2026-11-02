//! The skills: one renderer for every skill Clauseo ships (R21's `stetdoc`, R22's three), with
//! the hash manifest and the frontmatter parser. Claude reads them from the plugin's `skills/`
//! folder, Codex from its home's `skills/` folder; the render is the same for both.
//!
//! A write touches only the skills it is given: their folders, their files and their manifest
//! entries. Every other folder (a lawyer's, Codex's own `.system`) and every other entry stays
//! byte for byte. [`render`] is pure (the actor may call it, or hand out what it rendered
//! once); [`write_rendered`] reads and writes files, so it runs off the host thread, before each
//! agent's launch. It is idempotent: nothing is written when everything is current.
//!
//! The write never follows a symlink inside the skills folder, re-checks every name and path it
//! is handed, and holds one process-wide lock, so two launches never write the same folder at
//! once. A write that fails leaves the agent's launch to go on: the host logs it as a warning
//! and never fails a launch on it (decision record 0030).

pub mod builtin;
pub mod frontmatter;
pub mod manifest;
pub mod stetdoc;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use crate::error::ProfileError;
use crate::write;
use manifest::{Entry, MANIFEST, Manifest, sha256_hex};

/// The text a rendered file may never hold: Claude Code runs `` !`cmd` `` when it loads a skill.
pub const SHELL_INJECTION: &str = "!`";

/// The most bytes a skill name has.
const NAME_MAX: usize = 64;

/// The most bytes one part of a file's path has.
const PART_MAX: usize = 255;

/// The file every skill has, whose frontmatter is `name` and `description` alone.
pub const SKILL_MD: &str = "SKILL.md";

/// Windows' reserved device names, refused as a name or path part whatever the case or
/// extension (`nul.md` opens the device too).
const DEVICE_NAMES: [&str; 4] = ["con", "prn", "aux", "nul"];

/// Held by every write, so two launch-prep threads never write the same skills folder at once.
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// The write lock; a write that panicked left nothing half done the next one cannot redo.
pub(crate) fn write_lock() -> MutexGuard<'static, ()> {
    WRITE_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One file of a skill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFile {
    /// Its path in the skill's folder, `/`-separated (`SKILL.md`, `references/x.md`).
    pub path: String,
    /// Its bytes, as rendered.
    pub bytes: Vec<u8>,
}

/// A skill ready to render.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillSource {
    /// The folder name and the skill's name (`^[a-z0-9-]{1,64}$`).
    pub name: String,
    /// The version its manifest entry records.
    pub version: String,
    /// Its files.
    pub files: Vec<SkillFile>,
}

impl SkillSource {
    /// A skill whose files are written as they are. [`render`] still refuses it unless its
    /// `SKILL.md` is already in the rendered form (`frontmatter::render` then the body).
    /// stetdoc's goes through [`stetdoc::skill_source`] instead.
    pub fn verbatim(name: &str, version: &str, files: Vec<SkillFile>) -> Self {
        Self {
            name: name.to_owned(),
            version: version.to_owned(),
            files,
        }
    }

    /// A built-in skill from its source files (R22's): `SKILL.md` must hold the fixed
    /// frontmatter with `name` equal to `name` and a `metadata.clauseo.version`; it is rendered
    /// with `name` and `description` alone. Other files go as they are.
    pub fn builtin(name: &str, files: Vec<SkillFile>) -> Result<Self, ProfileError> {
        let mut version = None;
        let mut rendered = Vec::with_capacity(files.len());
        for file in files {
            if file.path != "SKILL.md" {
                rendered.push(file);
                continue;
            }
            let text = std::str::from_utf8(&file.bytes).map_err(|err| {
                ProfileError::skill(name, format!("SKILL.md is not UTF-8: {err}"))
            })?;
            let (front, body) =
                frontmatter::parse(text).map_err(|reason| ProfileError::skill(name, reason))?;
            if front.name != name {
                return Err(ProfileError::skill(
                    name,
                    format!("its `name` is `{}`, not its folder's", front.name),
                ));
            }
            version = match front.clauseo.get("version") {
                Some(frontmatter::MetaValue::Scalar(version)) => Some(version.clone()),
                _ => return Err(ProfileError::skill(name, "no metadata.clauseo.version")),
            };
            let text = format!("{}{body}", frontmatter::render(name, &front.description));
            rendered.push(SkillFile {
                path: file.path,
                bytes: text.into_bytes(),
            });
        }
        let version = version.ok_or_else(|| ProfileError::skill(name, "no SKILL.md"))?;
        Ok(Self {
            name: name.to_owned(),
            version,
            files: rendered,
        })
    }
}

/// One skill as rendered: its files and its manifest entry. Only [`render`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSkill {
    name: String,
    files: Vec<SkillFile>,
    entry: Entry,
}

impl RenderedSkill {
    /// The skill's name, its folder's.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its files.
    pub fn files(&self) -> &[SkillFile] {
        &self.files
    }

    /// Its manifest entry: the version and each file's SHA-256.
    pub fn entry(&self) -> &Entry {
        &self.entry
    }
}

/// Skills checked and ready to write, with their manifest entries. Only [`render`] and
/// [`render_each`] make one with skills in it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    skills: Vec<RenderedSkill>,
}

impl Rendered {
    /// The skills, in the order given.
    pub fn skills(&self) -> &[RenderedSkill] {
        &self.skills
    }
}

/// What a write did, as paths under the root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenderReport {
    /// Files written (new, changed or drifted).
    pub written: Vec<PathBuf>,
    /// Files changed or deleted on disk since the last write; each was logged and rewritten.
    pub drifted: Vec<PathBuf>,
    /// Files the last write recorded that the skill no longer has, now removed.
    pub removed: Vec<PathBuf>,
}

/// Checks `skills` and computes their manifest entries: pure, with no file read and no
/// process. Refuses the whole set when one skill has a bad name or path, no `SKILL.md` in the
/// rendered form, a file holding [`SHELL_INJECTION`], or a name given twice.
pub fn render(skills: &[SkillSource]) -> Result<Rendered, ProfileError> {
    let (rendered, mut refused) = render_each(skills);
    if refused.is_empty() {
        Ok(rendered)
    } else {
        Err(refused.swap_remove(0))
    }
}

/// [`render`], one skill at a time: a skill [`render`] would refuse is left out with a
/// warning and its error returned, and the others are rendered. So stetdoc's text, which is not
/// Clauseo's, can fail only stetdoc's skill.
pub fn render_each(skills: &[SkillSource]) -> (Rendered, Vec<ProfileError>) {
    let mut rendered = Vec::with_capacity(skills.len());
    let mut refused = Vec::new();
    for (at, skill) in skills.iter().enumerate() {
        let given_twice = skills
            .get(..at)
            .unwrap_or_default()
            .iter()
            .any(|seen| seen.name == skill.name);
        let checked = if given_twice {
            Err(ProfileError::skill(&skill.name, "the name is given twice"))
        } else {
            check(skill)
        };
        if let Err(error) = checked {
            tracing::warn!(%error, "a skill is left out");
            refused.push(error);
            continue;
        }
        let files = skill
            .files
            .iter()
            .map(|file| (file.path.clone(), sha256_hex(&file.bytes)))
            .collect();
        rendered.push(RenderedSkill {
            name: skill.name.clone(),
            files: skill.files.clone(),
            entry: Entry {
                version: skill.version.clone(),
                files,
            },
        });
    }
    (Rendered { skills: rendered }, refused)
}

/// Writes `rendered` into `root` (a skills folder): each skill's files at
/// `root/<name>/<path>`, each atomically, and its entry in `root/.clauseo-manifest.json`.
///
/// - First the temp files a crashed write left in `root` and in each skill's folder go.
/// - Then, per skill, the files the old entry lists that the skill no longer has are removed,
///   before anything is written (so a file renamed only in case survives on a disk that ignores
///   case).
/// - A file whose bytes on disk differ from what the manifest recorded, or that is gone, has
///   drifted: a warning names it and it is written again.
/// - Every other folder and every other manifest entry stays as it was.
///
/// Every name and path is checked again, and a symlink anywhere under `root` on a path the
/// write would take is an error: nothing is written or removed through one. One process-wide
/// lock serialises writes. Reads and writes files: off the host thread.
pub fn write_rendered(root: &Path, rendered: &Rendered) -> Result<RenderReport, ProfileError> {
    let _held = write_lock();
    write_unlocked(root, rendered)
}

/// [`write_rendered`], with the lock already held.
pub(crate) fn write_unlocked(
    root: &Path,
    rendered: &Rendered,
) -> Result<RenderReport, ProfileError> {
    for skill in &rendered.skills {
        recheck(root, skill)?;
    }
    no_links(root, &[MANIFEST])?;
    // A linked skill folder is refused before the sweep, so it is never entered.
    for skill in &rendered.skills {
        no_links(root, &[skill.name.as_str()])?;
    }
    write::sweep_temps(root, false);
    for skill in &rendered.skills {
        write::sweep_temps(&root.join(&skill.name), true);
    }
    let mut manifest = Manifest::read(root)?;
    let mut report = RenderReport::default();
    for skill in &rendered.skills {
        let folder = root.join(&skill.name);
        let old = manifest.entry(&skill.name).unwrap_or_default();
        for path in old
            .files
            .keys()
            .filter(|path| !skill.entry.files.contains_key(*path))
        {
            if !is_safe_path(path) {
                tracing::warn!(
                    path,
                    "a manifest entry names an unsafe path; it is left alone"
                );
                continue;
            }
            let parts = path_parts(&skill.name, path);
            no_links(root, &parts)?;
            let target = folder.join(path);
            write::remove_file(&target)?;
            report.removed.push(target);
        }
        for file in &skill.files {
            no_links(root, &path_parts(&skill.name, &file.path))?;
            let target = folder.join(&file.path);
            let drifted = old.files.get(&file.path).is_some_and(|recorded| {
                std::fs::read(&target).map_or(true, |held| sha256_hex(&held) != *recorded)
            });
            if drifted {
                tracing::warn!(
                    path = %target.display(),
                    "a skill file changed or went since Clauseo wrote it; it is written again"
                );
                report.drifted.push(target.clone());
            }
            if write::replace_if_differs(&target, &file.bytes)? {
                report.written.push(target);
            }
        }
        manifest.set(&skill.name, &skill.entry);
    }
    write::replace_if_differs(&root.join(MANIFEST), &manifest.to_bytes())?;
    Ok(report)
}

/// Whether `skill`'s files in `root` are what its manifest entry records for `version`: the
/// entry has that version, and every file it lists is on disk with its hash, reached through no
/// symlink. A render that would change nothing can be skipped (stetdoc's: no need to run
/// `skills get core`).
pub fn is_current(root: &Path, skill: &str, version: &str) -> Result<bool, ProfileError> {
    if !is_skill_name(skill) || no_links(root, &[MANIFEST]).is_err() {
        return Ok(false);
    }
    let Some(entry) = Manifest::read(root)?.entry(skill) else {
        return Ok(false);
    };
    Ok(entry.version == version
        && !entry.files.is_empty()
        && entry.files.iter().all(|(path, hash)| {
            is_safe_path(path)
                && no_links(root, &path_parts(skill, path)).is_ok()
                && std::fs::read(root.join(skill).join(path))
                    .is_ok_and(|held| sha256_hex(&held) == *hash)
        }))
}

/// Whether `name` is a skill name: `^[a-z0-9-]{1,64}$`, and no Windows device name.
pub fn is_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        && !is_device_name(name)
}

/// Whether `path` is a plain relative path inside a skill's folder: `/`-separated parts, each
/// of `[A-Za-z0-9._-]`, none empty, none starting or ending with `.` (so no `.`, no `..`, no
/// hidden file, nothing Windows would trim), none a Windows device name. So no root, no `\\`,
/// no drive and no alternate data stream (`:`) either.
pub fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && path.split('/').all(|part| {
            !part.is_empty()
                && part.len() <= PART_MAX
                && part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
                && !part.starts_with('.')
                && !part.ends_with('.')
                && !is_device_name(part)
        })
}

/// Whether `part` names a Windows device: `con`, `prn`, `aux`, `nul`, `com0`-`com9` or
/// `lpt0`-`lpt9`, in any case, with or without an extension.
fn is_device_name(part: &str) -> bool {
    let stem = part
        .split('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if DEVICE_NAMES.contains(&stem.as_str()) {
        return true;
    }
    let bytes = stem.as_bytes();
    bytes.len() == 4
        && (stem.starts_with("com") || stem.starts_with("lpt"))
        && bytes.get(3).is_some_and(u8::is_ascii_digit)
}

/// `name`, then `path`'s parts.
fn path_parts<'a>(name: &'a str, path: &'a str) -> Vec<&'a str> {
    std::iter::once(name).chain(path.split('/')).collect()
}

/// An error when `base` joined with a leading run of `parts` is a symlink. Stops at the first
/// part that does not exist yet: nothing under it can be a link.
pub(crate) fn no_links(base: &Path, parts: &[&str]) -> Result<(), ProfileError> {
    let mut path = base.to_owned();
    for part in parts {
        path.push(part);
        match std::fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                return Err(ProfileError::path(
                    &path,
                    "a symlink inside a skills folder; Clauseo never writes through one",
                ));
            }
            Ok(_) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(ProfileError::io("read", &path, err)),
        }
    }
    Ok(())
}

/// [`check`]'s name and path rules again, at write time, on what a [`Rendered`] holds.
fn recheck(root: &Path, skill: &RenderedSkill) -> Result<(), ProfileError> {
    let safe = is_skill_name(&skill.name)
        && skill
            .files
            .iter()
            .all(|file| is_safe_path(&file.path) && skill.entry.files.contains_key(&file.path))
        && skill.entry.files.len() == skill.files.len();
    if safe {
        Ok(())
    } else {
        Err(ProfileError::path(
            &root.join(&skill.name),
            "a rendered skill with a bad name or path; nothing is written",
        ))
    }
}

/// Refuses a bad name, a bad path or two paths that differ only in case, no `SKILL.md`, a
/// `SKILL.md` not in the rendered form (`name` equal to the skill's and `description`, nothing
/// else), or a shell injection.
fn check(skill: &SkillSource) -> Result<(), ProfileError> {
    let refuse = |reason: String| Err(ProfileError::skill(&skill.name, reason));
    if !is_skill_name(&skill.name) {
        return refuse("the name is not ^[a-z0-9-]{1,64}$, or is a Windows device name".into());
    }
    let mut seen = BTreeSet::new();
    for file in &skill.files {
        if !is_safe_path(&file.path) || file.path == MANIFEST {
            return refuse(format!(
                "the path `{}` is not a plain relative path",
                file.path
            ));
        }
        if !seen.insert(file.path.to_ascii_lowercase()) {
            return refuse(format!(
                "the path `{}` is given twice, or twice in different cases",
                file.path
            ));
        }
        if file
            .bytes
            .windows(SHELL_INJECTION.len())
            .any(|window| window == SHELL_INJECTION.as_bytes())
        {
            return refuse(format!("`{}` holds \"{SHELL_INJECTION}\"", file.path));
        }
    }
    let Some(skill_md) = skill.files.iter().find(|file| file.path == SKILL_MD) else {
        return refuse("it has no SKILL.md".into());
    };
    let text = std::str::from_utf8(&skill_md.bytes);
    let rendered_form = text.is_ok_and(|text| {
        frontmatter::parse(text).is_ok_and(|(front, _)| {
            front.name == skill.name
                && front.clauseo.is_empty()
                && text.starts_with(&frontmatter::render(&skill.name, &front.description))
        })
    });
    if !rendered_form {
        return refuse(
            "its SKILL.md frontmatter is not `name` (the skill's) and `description` alone".into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A `Rendered` built by hand (only this crate can) with a name or path that escapes the
    /// folder: the write refuses it before touching the disk.
    #[test]
    fn a_hand_built_rendered_with_an_escaping_path_is_refused_by_the_write() {
        let root = std::env::temp_dir().join("clauseo-profile-never-made");
        for (name, path) in [
            ("../x", "SKILL.md"),
            ("alpha", "../../x"),
            ("alpha", "/etc/x"),
        ] {
            let files = vec![SkillFile {
                path: path.to_owned(),
                bytes: b"x".to_vec(),
            }];
            let entry = Entry {
                version: "1".to_owned(),
                files: [(path.to_owned(), sha256_hex(b"x"))].into(),
            };
            let rendered = Rendered {
                skills: vec![RenderedSkill {
                    name: name.to_owned(),
                    files,
                    entry,
                }],
            };
            let error = write_rendered(&root, &rendered).unwrap_err();
            assert!(matches!(error, ProfileError::Path { .. }), "{error}");
            assert!(!root.exists());
        }
    }
}
