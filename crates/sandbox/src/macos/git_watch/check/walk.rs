//! The walk under every scan: each folder opened from the one above it, never through a link.

use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use super::super::{Fs, Kind, Meta, is_git};
use super::Entries;

/// How deep a walk holds each folder above it open; below that, a folder is opened again from
/// its path, so a deep tree never runs out of descriptors.
const HELD_MAX: usize = 32;

/// A folder a walk will visit: its path, its parent held open (unless past [`HELD_MAX`]) and
/// its depth.
type Queued<D> = (PathBuf, Option<Rc<D>>, usize);

/// What a walk does after a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Next {
    /// Goes below it.
    Below,
    /// Goes on, but not below it.
    Past,
    /// Stops the whole walk.
    Stop,
}

/// What a walk calls: `skip` before a folder is listed (true: pass it by), `folder` with each
/// listing, `unreadable` for a folder that can't be opened or listed (not one missing, nor one
/// on a volume a scan never enters), `refused` for a folder on a volume a scan never enters (a
/// network volume, an automount, `/net`), and `progress` once per folder listed.
pub(super) struct Visit<'a> {
    pub(super) skip: &'a dyn Fn(&Path, Meta) -> bool,
    pub(super) folder: &'a mut dyn FnMut(&Path, Meta, &Entries) -> Next,
    pub(super) unreadable: &'a mut dyn FnMut(&Path, &io::Error) -> Next,
    pub(super) refused: &'a mut dyn FnMut(&Path) -> Next,
    pub(super) progress: &'a dyn Fn(),
}

/// Visits folder `top` and every folder below it with its file ID and listing, never through
/// a link, never into a `.git`, a folder `skip` names, or a folder [`Fs::open_dir`] refuses (a
/// network volume, an automount, `/net`), logged. Each folder is listed through its own held
/// handle and its subfolders are opened from it, so one swapped for a link after the listing
/// is never followed. `visit` says whether to go below the folder.
#[cfg(test)]
pub(in super::super) fn walk<F: Fs>(
    fs: &F,
    top: &Path,
    skip: &dyn Fn(&Path, Meta) -> bool,
    visit: &mut dyn FnMut(&Path, Meta, &Entries) -> bool,
) {
    let mut folder = |dir: &Path, meta: Meta, entries: &Entries| {
        if visit(dir, meta, entries) {
            Next::Below
        } else {
            Next::Past
        }
    };
    let mut visit = Visit {
        skip,
        folder: &mut folder,
        unreadable: &mut |_, _| Next::Past,
        refused: &mut |_| Next::Past,
        progress: &|| {},
    };
    walk_with(fs, top, &mut visit);
}

/// [`walk`], calling `visit`'s hooks.
pub(super) fn walk_with<F: Fs>(fs: &F, top: &Path, visit: &mut Visit<'_>) {
    let mut folders: Vec<Queued<F::Dir>> = vec![(top.to_owned(), None, 0)];
    while let Some((dir, above, depth)) = folders.pop() {
        let opened = match (&above, dir.file_name()) {
            (Some(above), Some(name)) => fs.open_child(above, name, &dir),
            _ => fs.open_dir(&dir),
        };
        let (held, meta) = match opened {
            Ok(Some(found)) => found,
            Ok(None) => continue,
            Err(error) if error.kind() == io::ErrorKind::Unsupported => {
                let shown = dir.display();
                tracing::info!(dir = %shown, "the git watch skips a network volume or an automount");
                if (visit.refused)(&dir) == Next::Stop {
                    return;
                }
                continue;
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                tracing::warn!(dir = %dir.display(), %error, "the git watch can't open a folder");
                if (visit.unreadable)(&dir, &error) == Next::Stop {
                    return;
                }
                continue;
            }
        };
        if (visit.skip)(&dir, meta) {
            continue;
        }
        let entries = match fs.entries(&held) {
            Ok(entries) => entries,
            Err(error) => {
                tracing::warn!(dir = %dir.display(), %error, "the git watch can't list a folder");
                if (visit.unreadable)(&dir, &error) == Next::Stop {
                    return;
                }
                continue;
            }
        };
        (visit.progress)();
        match (visit.folder)(&dir, meta, &entries) {
            Next::Below => {}
            Next::Past => continue,
            Next::Stop => return,
        }
        let held = Rc::new(held);
        for (name, kind) in entries {
            if kind == Kind::Dir && !is_git(&name) {
                let above = (depth < HELD_MAX).then(|| Rc::clone(&held));
                folders.push((dir.join(name), above, depth + 1));
            }
        }
    }
}
