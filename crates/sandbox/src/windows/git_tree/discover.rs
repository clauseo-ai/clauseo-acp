//! Steps 2 and 3 of `git_tree.rs`: the per-machine Git for Windows, refused when it is per-user,
//! on a network path, behind a reparse point, in a folder others than administrators can change,
//! or holding a reparse point or a folder or file others can change; its walk and its tree id.
//! Pure: tested on every OS with the fake.

use std::path::{Path, PathBuf};

use super::{
    Entry, FileId, GIT_EXE, MARKER, MSYS_DLL, Refused, Source, TreeFs, Unavailable, digest, joined,
    tree_id,
};
use crate::windows::admin_copy::Found;

/// Steps 2 and 3: the per-machine Git, as the module says, with its walk and its tree id.
pub(super) fn discover(fs: &mut impl TreeFs) -> Result<Source, Unavailable> {
    let registered = fs.registry_install().map_err(|why| {
        tracing::warn!(%why, "Git for Windows' registry key couldn't be read");
        Unavailable::SourceUnreadable
    })?;
    let root = if let Some(path) = registered.filter(|path| !path.trim().is_empty()) {
        PathBuf::from(path.trim())
    } else {
        let program_files = fs
            .program_files()
            .map_err(|_why| Unavailable::SourceUnreadable)?;
        let fallback = program_files.join("Git");
        if fs.found(&fallback).ok() == Some(Found::Missing) {
            return Err(Unavailable::NoGit);
        }
        fallback
    };
    if !local_absolute(&root) || fs.is_network(&root).unwrap_or(true) {
        return Err(Unavailable::NetworkPath);
    }
    let profiles = fs
        .profiles_dir()
        .map_err(|_why| Unavailable::SourceUnreadable)?;
    if inside(&root, &profiles) {
        return Err(Unavailable::PerUserGit);
    }
    match fs.plain_chain(&root) {
        Ok(()) => {}
        Err(Refused::Reparse) => return Err(Unavailable::ReparsePoint),
        Err(Refused::Other(why)) => {
            tracing::warn!(%why, "the Git install's folders couldn't be opened");
            return Err(Unavailable::NoGit);
        }
        Err(Refused::Changed) => return Err(Unavailable::SourceUnreadable),
    }
    if !fs.admin_only(&root).unwrap_or(false) {
        return Err(Unavailable::PerUserGit);
    }
    let msys = identity(fs, &joined(&root, &MSYS_DLL))?;
    let git_exe = joined(&root, &GIT_EXE);
    let git = identity(fs, &git_exe)?;
    let version = fs.file_version(&git_exe).ok().flatten();
    let listing = listing(fs, &root)?;
    let id = tree_id(&root, version.as_deref(), [&msys, &git], &digest(&listing));
    Ok(Source {
        root,
        id,
        files: [msys, git],
        listing,
    })
}

/// The install's walk: every folder and file in it, each one only administrators can change
/// (`level_safe` through its own handle), else `source-unsafe`, and none a reparse point. A
/// [`MARKER`] at its root is left out: the copy writes its own.
fn listing(fs: &mut impl TreeFs, root: &Path) -> Result<Vec<Entry>, Unavailable> {
    let entries = fs.walk(root).map_err(|refused| match refused {
        Refused::Reparse => Unavailable::ReparsePoint,
        Refused::Other(why) => {
            tracing::warn!(%why, "the Git install couldn't be listed");
            Unavailable::SourceUnreadable
        }
        Refused::Changed => Unavailable::SourceUnreadable,
    })?;
    if entries.iter().any(|entry| entry.reparse) {
        return Err(Unavailable::ReparsePoint);
    }
    if let Some(entry) = entries.iter().find(|entry| !entry.admin_only) {
        let rel = entry.rel.display();
        tracing::warn!(entry = %rel, "the Git install holds one others can change");
        return Err(Unavailable::SourceUnsafe);
    }
    Ok(entries
        .into_iter()
        .filter(|entry| !entry.rel.as_os_str().eq_ignore_ascii_case(MARKER))
        .collect())
}

/// A file the install must hold: its identity, `NoGit` when it isn't there.
pub(super) fn identity(fs: &mut impl TreeFs, path: &Path) -> Result<FileId, Unavailable> {
    match fs.file_id(path) {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err(Unavailable::NoGit),
        Err(Refused::Reparse) => Err(Unavailable::ReparsePoint),
        Err(Refused::Other(why)) => {
            tracing::warn!(%why, "a Git file couldn't be read");
            Err(Unavailable::SourceUnreadable)
        }
        Err(Refused::Changed) => Err(Unavailable::SourceUnreadable),
    }
}

/// Whether `path` is a full path from a drive letter (`C:\...`), never UNC or a device path.
fn local_absolute(path: &Path) -> bool {
    let text = path.to_string_lossy().replace('/', "\\");
    let bytes = text.as_bytes();
    bytes.len() >= 3
        && bytes.first().is_some_and(u8::is_ascii_alphabetic)
        && bytes.get(1) == Some(&b':')
        && bytes.get(2) == Some(&b'\\')
        && !text.split('\\').any(|part| part == "." || part == "..")
}

/// Whether `path` is `dir` or below it, ignoring case.
fn inside(path: &Path, dir: &Path) -> bool {
    let text = |path: &Path| path.to_string_lossy().replace('/', "\\").to_lowercase();
    let (path, dir) = (text(path), text(dir));
    let dir = dir.trim_end_matches('\\');
    !dir.is_empty() && (path == dir || path.starts_with(&format!("{dir}\\")))
}
