//! The skills: one renderer for every skill Clauseo ships (R21's `stetdoc`, R22's three), with
//! the hash manifest and the frontmatter parser. Claude reads them from the plugin's `skills/`
//! folder, Codex from its home's `skills/` folder; the render is the same for both.
//!
//! A write touches only the skills it is given: their folders, their files and their manifest
//! entries. Every other folder (a lawyer's, Codex's own `.system`) and every other entry stays
//! byte for byte. [`render`] is pure (the actor may call it, or hand out what it rendered
//! once); [`write_rendered`] reads and writes files, so it runs off the host thread, before each
//! agent's launch. It is idempotent: nothing is written when everything is current.

pub mod frontmatter;
pub mod manifest;
pub mod stetdoc;

use std::path::{Component, Path, PathBuf};

use crate::error::ProfileError;
use crate::write;
use manifest::{Entry, MANIFEST, Manifest, sha256_hex};

/// The text a rendered file may never hold: Claude Code runs `` !`cmd` `` when it loads a skill.
pub const SHELL_INJECTION: &str = "!`";

/// The most bytes a skill name has.
const NAME_MAX: usize = 64;

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
    /// A skill whose files are written as they are (stetdoc's, whose text is stetdoc's own).
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

/// One skill as rendered: its files and its manifest entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedSkill {
    /// The skill's name, its folder's.
    pub name: String,
    /// Its files.
    pub files: Vec<SkillFile>,
    /// Its manifest entry: the version and each file's SHA-256.
    pub entry: Entry,
}

/// Skills checked and ready to write, with their manifest entries.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Rendered {
    /// The skills, in the order given.
    pub skills: Vec<RenderedSkill>,
}

/// What a write did, as paths under the root.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RenderReport {
    /// Files written (new, changed or drifted).
    pub written: Vec<PathBuf>,
    /// Files changed on disk since the last write; each was logged and rewritten.
    pub drifted: Vec<PathBuf>,
    /// Files the last write recorded that the skill no longer has, now removed.
    pub removed: Vec<PathBuf>,
}

/// Checks `skills` and computes their manifest entries: pure, with no file read and no
/// process. Refuses a skill with a bad name or path, no files, a file holding
/// [`SHELL_INJECTION`], or a name given twice.
pub fn render(skills: &[SkillSource]) -> Result<Rendered, ProfileError> {
    let mut rendered = Vec::with_capacity(skills.len());
    for (at, skill) in skills.iter().enumerate() {
        check(skill)?;
        if skills
            .get(..at)
            .unwrap_or_default()
            .iter()
            .any(|seen| seen.name == skill.name)
        {
            return Err(ProfileError::skill(&skill.name, "the name is given twice"));
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
    Ok(Rendered { skills: rendered })
}

/// Writes `rendered` into `root` (a skills folder): each skill's files at
/// `root/<name>/<path>`, each atomically, and its entry in `root/.clauseo-manifest.json`. A
/// file whose bytes on disk differ from what the manifest recorded has drifted: a warning names
/// it and it is written again. Files the old entry lists that the skill no longer has are
/// removed. Every other folder and every other manifest entry stays as it was. Reads and writes
/// files: off the host thread.
pub fn write_rendered(root: &Path, rendered: &Rendered) -> Result<RenderReport, ProfileError> {
    let mut manifest = Manifest::read(root)?;
    let mut report = RenderReport::default();
    for skill in &rendered.skills {
        let folder = root.join(&skill.name);
        let old = manifest.entry(&skill.name).unwrap_or_default();
        for file in &skill.files {
            let target = folder.join(&file.path);
            let drifted = old.files.get(&file.path).is_some_and(|recorded| {
                std::fs::read(&target).is_ok_and(|held| sha256_hex(&held) != *recorded)
            });
            if drifted {
                tracing::warn!(
                    path = %target.display(),
                    "a skill file changed since Clauseo wrote it; it is written again"
                );
                report.drifted.push(target.clone());
            }
            if write::replace_if_differs(&target, &file.bytes)? {
                report.written.push(target);
            }
        }
        for path in old
            .files
            .keys()
            .filter(|path| !skill.entry.files.contains_key(*path))
        {
            if is_safe_path(path) {
                let target = folder.join(path);
                write::remove_file(&target)?;
                report.removed.push(target);
            }
        }
        manifest.set(&skill.name, &skill.entry);
    }
    write::replace_if_differs(&root.join(MANIFEST), &manifest.to_bytes())?;
    Ok(report)
}

/// Whether `skill`'s files in `root` are what its manifest entry records for `version`: the
/// entry has that version, and every file it lists is on disk with its hash. A render that
/// would change nothing can be skipped (stetdoc's: no need to run `skills get core`).
pub fn is_current(root: &Path, skill: &str, version: &str) -> Result<bool, ProfileError> {
    let Some(entry) = Manifest::read(root)?.entry(skill) else {
        return Ok(false);
    };
    Ok(entry.version == version
        && !entry.files.is_empty()
        && entry.files.iter().all(|(path, hash)| {
            is_safe_path(path)
                && std::fs::read(root.join(skill).join(path))
                    .is_ok_and(|held| sha256_hex(&held) == *hash)
        }))
}

/// Whether `name` is a skill name: `^[a-z0-9-]{1,64}$`.
pub fn is_skill_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= NAME_MAX
        && name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
}

/// Whether `path` is a plain relative path inside a skill's folder: `/`-separated, no `..`, no
/// root, no `\`, no empty part.
fn is_safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && path.split('/').all(|part| !part.is_empty())
        && Path::new(path)
            .components()
            .all(|part| matches!(part, Component::Normal(_)))
}

/// Refuses a bad name, a bad path or a shell injection.
fn check(skill: &SkillSource) -> Result<(), ProfileError> {
    if !is_skill_name(&skill.name) {
        return Err(ProfileError::skill(
            &skill.name,
            "the name is not ^[a-z0-9-]{1,64}$",
        ));
    }
    if skill.files.is_empty() {
        return Err(ProfileError::skill(&skill.name, "it has no files"));
    }
    for file in &skill.files {
        if !is_safe_path(&file.path) || file.path == MANIFEST {
            return Err(ProfileError::skill(
                &skill.name,
                format!("the path `{}` is not a plain relative path", file.path),
            ));
        }
        if file
            .bytes
            .windows(SHELL_INJECTION.len())
            .any(|window| window == SHELL_INJECTION.as_bytes())
        {
            return Err(ProfileError::skill(
                &skill.name,
                format!("`{}` holds \"{SHELL_INJECTION}\"", file.path),
            ));
        }
    }
    Ok(())
}
