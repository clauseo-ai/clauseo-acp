//! Claude's plugin folder, `<data>/profile/claude-plugin`, holds only what the profile ships
//! ([`match_plugin`]): the plugin manifest, the skills folder's manifest and the files of the
//! skills a launch renders, each with exactly the bytes the profile renders. Claude Code loads
//! the whole folder at every launch, in every matter, and Codex's skills index names files in
//! it, so anything else in it goes before an agent starts. The host runs it after each launch's
//! write ([`crate::write_skills`]), written or not, and refuses the launch when it fails.
//!
//! What goes is first renamed aside into the plugin's parent, `<data>/profile`, under a name
//! starting [`ASIDE`] (Claude Code loads nothing from there), then removed. One that cannot be
//! removed stays aside until a later match removes it; only one that can be neither renamed nor
//! removed is an error.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::error::ProfileError;
use crate::skills::manifest::{MANIFEST, Manifest};
use crate::{PLUGIN_MANIFEST, PLUGIN_MANIFEST_PATH, SkillWrite, skills, write};

/// The start of the name an entry gets when it is set aside in the plugin's parent.
pub(crate) const ASIDE: &str = ".clauseo-aside-";

/// Numbers the aside names of one process, so two never collide.
static NEXT_ASIDE: AtomicU64 = AtomicU64::new(0);

/// Makes the plugin folder of `skills` (a write with a plugin, Claude's: none for any other)
/// hold only what that write puts there: [`PLUGIN_MANIFEST_PATH`], the skills folder's
/// manifest (with the rendered skills' entries alone), and each file of each rendered skill. A
/// file is kept only when it is a plain file with one link whose bytes are the rendered ones;
/// every other entry goes (a file, a symlink or junction, never followed, any other kind, and a
/// real folder with all it holds unless a kept file lies under it), and each kept file that
/// went or was missing is written again. First the plugin folder and its parent are made real
/// folders ([`real_folders`]). Under the skills write lock. Returns how many entries went; an
/// error when a folder cannot be read, an entry can be neither set aside nor removed, or a file
/// cannot be written, so the folder may not hold what the profile ships. Reads and changes
/// files: off the host thread.
pub fn match_plugin(skills: &SkillWrite) -> Result<usize, ProfileError> {
    let Some(plugin) = skills.plugin.as_deref() else {
        return Ok(0);
    };
    let _held = skills::write_lock();
    let parent = real_folders(plugin)?;
    let left = sweep_aside(parent);
    if left > 0 {
        tracing::warn!(
            left,
            "entries set aside from the built-in plugin are still there"
        );
    }
    let shipped = shipped_files(plugin, skills);
    let mut folders = BTreeSet::new();
    for file in shipped.keys() {
        for above in file.ancestors().skip(1) {
            if above == plugin || !above.starts_with(plugin) {
                break;
            }
            folders.insert(above.to_path_buf());
        }
    }
    let mut moved = 0;
    let found = Found {
        shipped: &shipped,
        folders: &folders,
        aside: parent,
    };
    // Checked again just before the sweep: a link put in place of either folder since
    // `real_folders` is an error, never swept through.
    for folder in [parent, plugin] {
        let meta = std::fs::symlink_metadata(folder)
            .map_err(|err| ProfileError::io("read", folder, err))?;
        if !is_real_dir(&meta) {
            return Err(ProfileError::path(
                folder,
                "the built-in plugin's folder became a link",
            ));
        }
    }
    found.sweep(plugin, &mut moved)?;
    for (path, bytes) in &shipped {
        if std::fs::symlink_metadata(path).is_err() {
            write::replace_if_differs(path, bytes)?;
        }
    }
    Ok(moved)
}

/// Makes the plugin folder `plugin` and its parent (`<data>/profile`) real folders, the parent
/// first: a symlink or junction there (any reparse point on Windows) or a file is removed
/// itself, never followed, and a folder made in its place; a missing one is made. Returns the
/// parent. Called before anything is written in or swept from the plugin, so nothing a link
/// leads to is ever changed. Under the skills write lock.
pub(crate) fn real_folders(plugin: &Path) -> Result<&Path, ProfileError> {
    let parent = plugin
        .parent()
        .ok_or_else(|| ProfileError::path(plugin, "the built-in plugin's folder has no parent"))?;
    for folder in [parent, plugin] {
        match std::fs::symlink_metadata(folder) {
            Ok(meta) if is_real_dir(&meta) => continue,
            Ok(_) => {
                tracing::warn!("a link or file stood at the built-in plugin's folder; it went");
                write::remove_entry(folder)?;
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => return Err(ProfileError::io("read", folder, err)),
        }
        write::create_dir_all(folder)?;
    }
    Ok(parent)
}

/// Whether `meta` (from `symlink_metadata`) is a folder that is no link or reparse point.
fn is_real_dir(meta: &std::fs::Metadata) -> bool {
    meta.is_dir() && !meta.file_type().is_symlink() && !crate::packs::read::is_reparse(meta)
}

/// The files `skills` writes under `plugin`, each joined part by part, with their bytes.
fn shipped_files<'a>(plugin: &Path, skills: &'a SkillWrite) -> BTreeMap<PathBuf, Cow<'a, [u8]>> {
    let mut shipped = BTreeMap::new();
    shipped.insert(
        PLUGIN_MANIFEST_PATH
            .iter()
            .fold(plugin.to_path_buf(), |at, part| at.join(part)),
        Cow::Borrowed(PLUGIN_MANIFEST.as_bytes()),
    );
    let mut manifest = Manifest::default();
    for skill in skills.rendered.skills() {
        manifest.set(skill.name(), skill.entry());
        let folder = skills.root.join(skill.name());
        for file in skill.files() {
            shipped.insert(
                file.path
                    .split('/')
                    .fold(folder.clone(), |at, part| at.join(part)),
                Cow::Borrowed(file.bytes.as_slice()),
            );
        }
    }
    shipped.insert(skills.root.join(MANIFEST), Cow::Owned(manifest.to_bytes()));
    shipped
}

/// What a sweep keeps and where what goes is set aside.
struct Found<'a> {
    /// Each shipped file with its bytes.
    shipped: &'a BTreeMap<PathBuf, Cow<'a, [u8]>>,
    /// The real folders above them.
    folders: &'a BTreeSet<PathBuf>,
    /// The plugin's parent, where entries are set aside.
    aside: &'a Path,
}

impl Found<'_> {
    /// Takes from `dir`, depth first, every entry that is neither a shipped file holding its
    /// bytes ([`holds_exactly`]) nor a real folder above one, counting them in `moved`. A missing
    /// `dir` is no error.
    fn sweep(&self, dir: &Path, moved: &mut usize) -> Result<(), ProfileError> {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(ProfileError::io("read", dir, err)),
        };
        for entry in entries {
            let entry = entry.map_err(|err| ProfileError::io("read", dir, err))?;
            let path = entry.path();
            // The entry's own kind: a symlink or a junction is never a folder here.
            let meta = std::fs::symlink_metadata(&path)
                .map_err(|err| ProfileError::io("read", &path, err))?;
            if is_real_dir(&meta) && self.folders.contains(&path) {
                self.sweep(&path, moved)?;
                continue;
            }
            let kept = meta.is_file()
                && self
                    .shipped
                    .get(&path)
                    .is_some_and(|bytes| holds_exactly(&path, bytes));
            if !kept {
                take(&path, self.aside)?;
                *moved += 1;
            }
        }
        Ok(())
    }
}

/// Whether the plain file at `path` has one link and holds exactly `bytes`.
fn holds_exactly(path: &Path, bytes: &[u8]) -> bool {
    link_count(path) == Some(1) && std::fs::read(path).is_ok_and(|held| held == bytes)
}

/// How many hard links the file at `path` has; none when that cannot be read.
#[cfg(unix)]
fn link_count(path: &Path) -> Option<u64> {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::symlink_metadata(path)
        .ok()
        .map(|meta| meta.nlink())
}

/// How many hard links the file at `path` has; none when that cannot be read.
#[cfg(windows)]
fn link_count(path: &Path) -> Option<u64> {
    crate::packs::windows::link_count(path).map(u64::from)
}

/// No link count off Unix and Windows: no file is kept, each is written again.
#[cfg(not(any(unix, windows)))]
fn link_count(_path: &Path) -> Option<u64> {
    None
}

/// How deep [`take`] goes into a folder it could not rename, removing entry by entry: a deeper
/// tree is an error, so the launch is refused rather than held up.
const MAX_TAKE_DEPTH: usize = 64;

/// The most entries left aside by earlier matches that one match removes; the rest wait for
/// later ones.
const MAX_SWEPT: usize = 256;

/// Takes the entry at `path` out of the plugin: renamed to a fresh [`ASIDE`] name in `aside`,
/// then removed with all it holds, a failure there left for a later [`sweep_aside`]. When the
/// rename fails, it is removed where it is (a real folder entry by entry, each the same way, at
/// most [`MAX_TAKE_DEPTH`] folders down); an error only when that fails too.
fn take(path: &Path, aside: &Path) -> Result<(), ProfileError> {
    take_at(path, aside, 0)
}

/// [`take`] of an entry `depth` folders below the first one taken.
fn take_at(path: &Path, aside: &Path, depth: usize) -> Result<(), ProfileError> {
    let to = aside.join(format!(
        "{ASIDE}{}-{}",
        std::process::id(),
        NEXT_ASIDE.fetch_add(1, Ordering::Relaxed)
    ));
    if std::fs::symlink_metadata(&to).is_err() && write::rename(path, &to).is_ok() {
        if write::remove_any(&to).is_err() {
            tracing::warn!("an entry set aside from the built-in plugin stays for now");
        }
        return Ok(());
    }
    let meta =
        std::fs::symlink_metadata(path).map_err(|err| ProfileError::io("read", path, err))?;
    if !is_real_dir(&meta) {
        return write::remove_entry(path);
    }
    if depth >= MAX_TAKE_DEPTH {
        return Err(ProfileError::path(
            path,
            "a folder in the built-in plugin is too deep to remove",
        ));
    }
    let entries = std::fs::read_dir(path).map_err(|err| ProfileError::io("read", path, err))?;
    for entry in entries {
        let entry = entry.map_err(|err| ProfileError::io("read", path, err))?;
        take_at(&entry.path(), aside, depth.saturating_add(1))?;
    }
    write::remove_dir(path)
}

/// Removes what earlier matches set aside in `aside` ([`ASIDE`] names), best-effort and at most
/// [`MAX_SWEPT`] of them; how many it found still there.
fn sweep_aside(aside: &Path) -> usize {
    let Ok(entries) = std::fs::read_dir(aside) else {
        return 0;
    };
    entries
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with(ASIDE))
        })
        .take(MAX_SWEPT)
        .filter(|entry| write::remove_any(&entry.path()).is_err())
        .count()
}
