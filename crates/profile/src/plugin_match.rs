//! Claude's plugin folder, `<data>/profile/claude-plugin`, holds only what the profile ships
//! ([`match_plugin`]): the plugin manifest, the skills folder's manifest and the files of the
//! skills a launch renders. Claude Code loads the whole folder at every launch, in every matter,
//! and Codex's skills index names files in it, so anything else in it goes before an agent
//! starts. The host runs it after each launch's write ([`crate::write_skills`]), written or
//! not, and refuses the launch when it fails.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::error::ProfileError;
use crate::skills::manifest::{MANIFEST, Manifest};
use crate::{PLUGIN_MANIFEST_PATH, SkillWrite, skills, write};

/// Makes the plugin folder of `skills` (a write with a plugin, Claude's: none for any other)
/// hold only what that write puts there: [`PLUGIN_MANIFEST_PATH`], the skills folder's
/// manifest, and each file of each rendered skill. Every other entry goes: a file, a symlink
/// (removed itself, never followed, whatever it is named), any other kind, and a real folder
/// once emptied, unless it holds a kept file; the manifest keeps only the rendered skills'
/// entries. Under the skills write lock. Returns how many entries other than folders it
/// removed; an error when a folder cannot be read or an entry cannot be removed, so the folder
/// may still hold something the profile did not ship. Reads and removes files: off the host
/// thread.
pub fn match_plugin(skills: &SkillWrite) -> Result<usize, ProfileError> {
    let Some(plugin) = skills.plugin.as_deref() else {
        return Ok(0);
    };
    let _held = skills::write_lock();
    let kept = kept_files(plugin, skills);
    let mut folders = BTreeSet::new();
    for file in &kept {
        for above in file.ancestors().skip(1) {
            if above == plugin || !above.starts_with(plugin) {
                break;
            }
            folders.insert(above.to_path_buf());
        }
    }
    let mut removed = 0;
    sweep(plugin, &kept, &folders, &mut removed)?;
    let root = &skills.root;
    let mut manifest = Manifest::read(root)?;
    let rendered = skills.rendered.skills();
    if manifest.retain_skills(|name| rendered.iter().any(|skill| skill.name() == name)) {
        write::replace_if_differs(&root.join(MANIFEST), &manifest.to_bytes())?;
    }
    Ok(removed)
}

/// The files `skills` writes under `plugin`, each joined part by part.
fn kept_files(plugin: &Path, skills: &SkillWrite) -> BTreeSet<PathBuf> {
    let mut kept = BTreeSet::new();
    kept.insert(
        PLUGIN_MANIFEST_PATH
            .iter()
            .fold(plugin.to_path_buf(), |at, part| at.join(part)),
    );
    kept.insert(skills.root.join(MANIFEST));
    for skill in skills.rendered.skills() {
        let folder = skills.root.join(skill.name());
        for file in skill.files() {
            kept.insert(
                file.path
                    .split('/')
                    .fold(folder.clone(), |at, part| at.join(part)),
            );
        }
    }
    kept
}

/// Removes from `dir`, depth first, every entry that is not one of `kept` (files) or `folders`
/// (the real folders above them), counting the entries other than folders in `removed`. A
/// missing `dir` is no error.
fn sweep(
    dir: &Path,
    kept: &BTreeSet<PathBuf>,
    folders: &BTreeSet<PathBuf>,
    removed: &mut usize,
) -> Result<(), ProfileError> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => return Err(ProfileError::io("read", dir, err)),
    };
    for entry in entries {
        let entry = entry.map_err(|err| ProfileError::io("read", dir, err))?;
        let path = entry.path();
        // The entry's own kind: a symlink or a junction is never a folder here.
        let kind = entry
            .file_type()
            .map_err(|err| ProfileError::io("read", &path, err))?;
        if kind.is_dir() {
            sweep(&path, kept, folders, removed)?;
            if !folders.contains(&path) {
                write::remove_dir(&path)?;
            }
        } else if !(kind.is_file() && kept.contains(&path)) {
            write::remove_entry(&path)?;
            *removed += 1;
        }
    }
    Ok(())
}
