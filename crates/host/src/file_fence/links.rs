//! What a search meets through a link below its root. Claude Code 2.1.293's `Grep` and `Glob`
//! run its embedded ripgrep without `--follow` and with `--no-config`, so neither follows a
//! symlink, a junction or a mount point's reparse point below the root (the evidence is in the
//! fence notes, `b4l-fencehook-notes.md`). That rests on a vendor's flags and on the embedded
//! ripgrep (a system one, `USE_BUILTIN_RIPGREP`, reads a config file that may add `--follow`),
//! so the fence does not rely on it: [`below`] walks the folder a search starts from without
//! following anything, resolves each link it meets ([`locate`], the network-safe walk) and
//! refuses when one leads into a fenced place, or to a place the walk can't vouch for. On Unix a
//! folder on another device (a mount point: a bind mount of the data folder, say) is judged
//! as a place of its own too. The walk runs inside the guard's deadline; past [`MAX_ENTRIES`]
//! it refuses, as the deadline would.

use std::path::{Path, PathBuf};

use super::decide::{Denial, Fence};
use super::real::{Ground, Spot, locate};

/// The most entries [`below`] reads before it refuses.
pub(crate) const MAX_ENTRIES: usize = 200_000;

/// The refusal a search from the local folder `root` earns through a link, junction or mount
/// point below it, if any (a link to a network location refused as [`Denial::Network`] when
/// `refuse_network`, else judged by its text). A file, or a folder that can't be read (the
/// search can't read it either), is not walked.
pub(crate) fn below(
    fence: &Fence,
    root: &Path,
    ground: &Ground<'_>,
    refuse_network: bool,
) -> Option<Denial> {
    let meta = std::fs::symlink_metadata(root).ok()?;
    if !meta.is_dir() {
        return None;
    }
    let mut stack: Vec<(PathBuf, u64)> = vec![(root.to_path_buf(), device(&meta))];
    let mut read = 0_usize;
    while let Some((folder, on)) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&folder) else {
            continue;
        };
        for entry in entries.flatten() {
            read = read.saturating_add(1);
            if read > MAX_ENTRIES {
                return Some(Denial::Late);
            }
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_symlink() {
                match locate(&path, ground) {
                    Spot::Local(real) => {
                        if let Some(kind) = fence.reached(&real) {
                            return Some(Denial::Holds(kind));
                        }
                    }
                    Spot::Unvouched => return Some(Denial::Unvouched),
                    Spot::Network(_) if refuse_network => return Some(Denial::Network),
                    Spot::Network(path) | Spot::Share(path) => {
                        if let Some(kind) = fence.reached_text(&path) {
                            return Some(Denial::Holds(kind));
                        }
                    }
                }
            } else if kind.is_dir() {
                let dev = entry.metadata().map_or(on, |meta| device(&meta));
                if dev != on
                    && let Some(kind) = fence.reached(&path)
                {
                    return Some(Denial::Holds(kind));
                }
                stack.push((path, dev));
            }
        }
    }
    None
}

/// The device a folder is on (Unix); one value elsewhere, where a mount point is a reparse
/// point the walk resolves as a link.
fn device(meta: &std::fs::Metadata) -> u64 {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt as _;
        meta.dev()
    }
    #[cfg(not(unix))]
    {
        let _ = meta;
        0
    }
}
