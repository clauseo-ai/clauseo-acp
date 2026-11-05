//! A launch's own Claude plugin of skill packs (decision record 0040), normally
//! `<data>/run/<session id>/skill-packs/`:
//!
//! ```text
//! skill-packs/
//!   .claude-plugin/plugin.json     named `clauseo-packs`
//!   skills/<pack>--<skill>/...     the launch's own copies, written from memory
//! ```
//!
//! The folder is new for each launch and goes with the run folder, so a session sees exactly
//! its matter's packs, and a pack turned off is never left loading. Each file is written from
//! the bytes [`super::inspect`] read, checked and hashed for this launch, with the default mode
//! (never an exec bit), and shares nothing with another launch's: a write through one launch's
//! files reaches no other session and no later launch. The folder is disposable, so nothing is
//! synced. Only [`write_packs`] makes the [`PacksWritten`] token that Claude's second `plugins`
//! entry and Codex's pack index need.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use super::SKILLS_DIR;
use super::render::Pack;
use crate::error::ProfileError;
use crate::skills::{SKILL_MD, is_safe_path, is_skill_name};
use crate::write;

/// The pack plugin's name, as Claude namespaces its skills (`clauseo-packs:<pack>--<skill>`).
pub const PLUGIN_NAME: &str = "clauseo-packs";

/// The pack plugin's manifest, `.claude-plugin/plugin.json`.
const PLUGIN_MANIFEST: &str = concat!(
    "{\n",
    "  \"name\": \"clauseo-packs\",\n",
    "  \"description\": \"The skill packs turned on for this matter.\"\n",
    "}\n"
);

/// Proof that [`write_packs`] wrote a launch's pack plugin: only it makes one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacksWritten {
    plugin: PathBuf,
    skills: Vec<WrittenSkill>,
}

/// One skill [`write_packs`] wrote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenSkill {
    /// Its rendered name, `<pack>--<skill>`.
    pub name: String,
    /// Its one-line description.
    pub description: String,
}

impl PacksWritten {
    /// The plugin's folder.
    pub fn plugin(&self) -> &Path {
        &self.plugin
    }

    /// Its `skills/` folder.
    pub fn skills_root(&self) -> PathBuf {
        self.plugin.join(SKILLS_DIR)
    }

    /// The skills written, packs in the order given, each pack's in its manifest's order.
    pub fn skills(&self) -> &[WrittenSkill] {
        &self.skills
    }

    /// A token naming `skills` under `plugin`, with nothing written: for the index's unit tests.
    #[cfg(test)]
    pub(super) fn for_test(plugin: &Path, skills: Vec<WrittenSkill>) -> Self {
        Self {
            plugin: plugin.to_owned(),
            skills,
        }
    }
}

/// Writes `packs` into a new plugin folder at `plugin` (which must not exist yet): the plugin
/// manifest, then each skill's files from the bytes in memory, one thread per pack. A pack
/// whose id came earlier in `packs` is left out with a warning. On any failure the folder is
/// removed and the error returned: the host logs it as a warning and launches without packs.
/// Writes files: on the launch-prep thread.
pub fn write_packs(plugin: &Path, packs: &[Pack]) -> Result<PacksWritten, ProfileError> {
    if let Some(parent) = plugin.parent() {
        write::create_dir_all(parent)?;
    }
    write::create_dir(plugin)?;
    let written = fill(plugin, packs);
    if written.is_err() {
        write::remove_dir_all(plugin);
    }
    written
}

/// [`write_packs`] into the folder it made.
fn fill(plugin: &Path, packs: &[Pack]) -> Result<PacksWritten, ProfileError> {
    write::write_new(
        &plugin.join(".claude-plugin").join("plugin.json"),
        PLUGIN_MANIFEST.as_bytes(),
    )?;
    let skills_root = plugin.join(SKILLS_DIR);
    write::create_dir(&skills_root)?;
    let mut ids = BTreeSet::new();
    let mut unique = Vec::with_capacity(packs.len());
    for pack in packs {
        if ids.insert(pack.id()) {
            unique.push(pack);
        } else {
            tracing::warn!("a skill pack is given twice; it is written once");
        }
    }
    for skill in unique.iter().flat_map(|pack| pack.skills()) {
        let safe = is_skill_name(skill.name())
            && skill.files().iter().all(|file| is_safe_path(&file.path))
            && skill.files().iter().any(|file| file.path == SKILL_MD);
        if !safe {
            return Err(ProfileError::path(
                &skills_root,
                "a skill pack's skill with a bad name or path; nothing is written",
            ));
        }
    }
    let root = skills_root.as_path();
    let results: Vec<Result<(), ProfileError>> = std::thread::scope(|scope| {
        // Every thread starts before the first is joined.
        let mut handles = Vec::with_capacity(unique.len());
        for pack in &unique {
            handles.push(scope.spawn(move || write_pack(root, pack)));
        }
        handles
            .into_iter()
            .map(|handle| {
                handle.join().unwrap_or_else(|_| {
                    Err(ProfileError::path(root, "a skill pack's write stopped"))
                })
            })
            .collect()
    });
    results.into_iter().collect::<Result<(), _>>()?;
    let skills = unique
        .iter()
        .flat_map(|pack| pack.skills())
        .map(|skill| WrittenSkill {
            name: skill.name().to_owned(),
            description: skill.description().to_owned(),
        })
        .collect();
    Ok(PacksWritten {
        plugin: plugin.to_owned(),
        skills,
    })
}

/// Every file of `pack`'s skills under `skills_root`, each a new file.
fn write_pack(skills_root: &Path, pack: &Pack) -> Result<(), ProfileError> {
    for skill in pack.skills() {
        let folder = skills_root.join(skill.name());
        for file in skill.files() {
            write::write_new(&folder.join(&file.path), &file.bytes)?;
        }
    }
    Ok(())
}
