//! A pack checked and rendered, in memory: the manifest, the layout, the scripts, and each
//! skill's `SKILL.md` rewritten to `name: <pack>--<skill>` and its `description` alone, as
//! stetdoc's is, so `allowed-tools`, `hooks` and every other key go. Then each skill goes
//! through the skills renderer's checks (`skills::check`). One refused skill refuses the pack.

use super::hash::{PackHash, content_hash};
use super::manifest::{self, ManifestError, PackManifest};
use super::read::{PackFile, PackRead};
use super::{PACK_MANIFEST, Refusal, SEPARATOR, SKILLS_DIR, refuse, scripts};
use crate::skills::builtin::is_unlisted_char;
use crate::skills::{SKILL_MD, SkillFile, SkillSource, check_scanning, frontmatter};

/// The files a pack may hold at its top, beside `clauseo-pack.json` and `skills/`.
pub const TOP_LEVEL: [&str; 5] = [
    "README.md",
    "LICENSE",
    "LICENSE.md",
    "LICENSE.txt",
    "NOTICE",
];

/// The most characters a skill's description has (Claude Code's own limit).
pub const DESCRIPTION_MAX: usize = 1024;

/// One skill of a pack, rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackSkill {
    name: String,
    description: String,
    files: Vec<SkillFile>,
}

impl PackSkill {
    /// Its rendered name, `<pack>--<skill>`, its folder's in the render.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Its description, one line, as its `SKILL.md` now holds it.
    pub fn description(&self) -> &str {
        &self.description
    }

    /// Its files, paths relative to its folder, `SKILL.md` rendered and the rest as read.
    pub fn files(&self) -> &[SkillFile] {
        &self.files
    }
}

/// A pack read, checked, hashed and rendered. Only [`super::inspect`] makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pack {
    manifest: PackManifest,
    hash: PackHash,
    skills: Vec<PackSkill>,
    scripts: bool,
}

impl Pack {
    /// Its manifest.
    pub fn manifest(&self) -> &PackManifest {
        &self.manifest
    }

    /// Its id.
    pub fn id(&self) -> &str {
        &self.manifest.id
    }

    /// Its content hash, over the bytes read.
    pub fn hash(&self) -> &PackHash {
        &self.hash
    }

    /// Its skills, rendered, in the manifest's order.
    pub fn skills(&self) -> &[PackSkill] {
        &self.skills
    }

    /// Whether it holds a script (copied, never run, never with the exec bit).
    pub fn has_scripts(&self) -> bool {
        self.scripts
    }
}

/// Checks the pack read from the folder `folder_id` and renders it.
pub(super) fn check_and_render(
    folder_id: &str,
    read: PackRead,
    allow_scripts: bool,
) -> Result<Pack, Refusal> {
    let PackRead { files, folders, .. } = read;
    let manifest_file = files
        .iter()
        .find(|file| file.path == PACK_MANIFEST)
        .ok_or(Refusal::Manifest)?;
    let manifest = manifest::parse(&manifest_file.bytes).map_err(|error| match error {
        ManifestError::BadId => Refusal::BadId,
        _ => Refusal::Manifest,
    })?;
    if manifest.id.as_bytes() != folder_id.as_bytes() {
        return Err(Refusal::IdMismatch);
    }
    let hash = content_hash(
        files
            .iter()
            .map(|file| (file.path.as_str(), file.bytes.as_slice())),
    );
    check_layout(&manifest, &files, &folders)?;
    let scripts = files.iter().any(scripts::is_script);
    if scripts && manifest.instruction_only {
        return Err(Refusal::ScriptInInstructionOnly);
    }
    if scripts && !allow_scripts {
        return Err(Refusal::Script);
    }
    let mut grouped: Vec<Vec<SkillFile>> = vec![Vec::new(); manifest.skills.len()];
    for file in files {
        if let Some((at, path)) = skill_of(&manifest, &file.path) {
            let path = path.to_owned();
            if let Some(group) = grouped.get_mut(at) {
                group.push(SkillFile {
                    path,
                    bytes: file.bytes,
                });
            }
        }
    }
    let skills = manifest
        .skills
        .iter()
        .zip(grouped)
        .map(|(name, files)| render_skill(&manifest.id, name, files))
        .collect::<Result<_, _>>()?;
    Ok(Pack {
        manifest,
        hash,
        skills,
        scripts,
    })
}

/// The index in the manifest's list of the skill whose folder holds `path`, and `path`
/// relative to that folder.
fn skill_of<'a>(manifest: &PackManifest, path: &'a str) -> Option<(usize, &'a str)> {
    let rest = path.strip_prefix(SKILLS_DIR)?.strip_prefix('/')?;
    let (skill, inner) = rest.split_once('/')?;
    let at = manifest.skills.iter().position(|name| name == skill)?;
    Some((at, inner))
}

/// Refuses a file or folder the layout does not allow: at the top, only the manifest,
/// [`TOP_LEVEL`] and `skills/`; under `skills/`, only the listed skills' folders, each with a
/// `SKILL.md`.
fn check_layout(
    manifest: &PackManifest,
    files: &[PackFile],
    folders: &[String],
) -> Result<(), Refusal> {
    for file in files {
        let allowed = file.path == PACK_MANIFEST
            || TOP_LEVEL.contains(&file.path.as_str())
            || skill_of(manifest, &file.path).is_some();
        if !allowed {
            return Err(Refusal::Unlisted);
        }
    }
    for folder in folders {
        let allowed = folder == SKILLS_DIR
            || folder
                .strip_prefix(SKILLS_DIR)
                .and_then(|rest| rest.strip_prefix('/'))
                .and_then(|rest| rest.split('/').next())
                .is_some_and(|skill| manifest.skills.iter().any(|name| name == skill));
        if !allowed {
            return Err(Refusal::Unlisted);
        }
    }
    for skill in &manifest.skills {
        let skill_md = format!("{SKILLS_DIR}/{skill}/{SKILL_MD}");
        if !files.iter().any(|file| file.path == skill_md) {
            return Err(Refusal::Unlisted);
        }
    }
    Ok(())
}

/// The skill `skill` of the pack `id`, from its files: its `SKILL.md` must be UTF-8, start
/// with a frontmatter whose `name` is `skill`, and hold a one-line description within
/// [`DESCRIPTION_MAX`] characters once joined and filtered. Rendered as `<id>--<skill>`, then
/// checked as every skill is, the shell-injection scan skipping binary templates.
fn render_skill(id: &str, skill: &str, mut files: Vec<SkillFile>) -> Result<PackSkill, Refusal> {
    let name = format!("{id}{SEPARATOR}{skill}");
    let skill_md = files
        .iter_mut()
        .find(|file| file.path == SKILL_MD)
        .ok_or(Refusal::Unlisted)?;
    let text = std::str::from_utf8(&skill_md.bytes).map_err(refuse(Refusal::Skill))?;
    let (description, body) =
        frontmatter::lenient_description(text).map_err(refuse(Refusal::Skill))?;
    let head = text.get(..text.len() - body.len()).ok_or(Refusal::Skill)?;
    if front_name(head) != Some(skill) {
        return Err(Refusal::Skill);
    }
    // One line for the rendered frontmatter (a literal `|` description holds newlines), through
    // the filter Codex's index uses: whitespace of any kind becomes one space, and every other
    // control, separator or format character goes, so none reaches Claude's YAML parser.
    let description = description
        .chars()
        .map(|c| if c.is_whitespace() { ' ' } else { c })
        .filter(|c| !is_unlisted_char(*c))
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if description.is_empty() || description.chars().count() > DESCRIPTION_MAX {
        return Err(Refusal::Skill);
    }
    let rendered = format!("{}{body}", frontmatter::render(&name, &description));
    skill_md.bytes = rendered.into_bytes();
    files.sort_unstable_by(|a, b| a.path.cmp(&b.path));
    let source = SkillSource::verbatim(&name, "", files);
    check_scanning(&source, scripts::is_text).map_err(refuse(Refusal::Skill))?;
    Ok(PackSkill {
        name,
        description,
        files: source.files,
    })
}

/// The top-level `name` of the frontmatter `head` (its text, `---` lines included): bare, or
/// in single or double quotes.
fn front_name(head: &str) -> Option<&str> {
    let value = head
        .lines()
        .find_map(|line| line.strip_prefix("name:"))?
        .trim();
    let unquoted = ['"', '\'']
        .iter()
        .find_map(|quote| value.strip_prefix(*quote)?.strip_suffix(*quote));
    Some(unquoted.unwrap_or(value))
}
