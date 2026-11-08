//! The folder a document sits in, as the held-open check looks at it ([`DocumentFolder`]): by
//! its path for the app's own matter paths ([`PathFolder`]), or through a handle a caller
//! opened without following any link for a path an agent named (the host's no-follow open), so
//! a link swapped in after the caller's check is never followed by the check's own looks.

use std::fs::File;
use std::io;
use std::path::Path;

/// What the held-open check ([`super::held_open`]) and the stale rule ask of the folder a
/// document sits in. Every look is at one entry of the folder by its name, never following a
/// link at it.
pub trait DocumentFolder {
    /// Whether the folder has an entry `name` (a link counts as itself, never followed).
    fn has(&self, name: &str) -> bool;
    /// Up to `max` of the folder's entry names that are UTF-8; `None` when it can't be listed.
    fn names(&self, max: usize) -> Option<Vec<String>>;
    /// The entry `name` opened for reading, never through a link; with `exclusive`, on Windows
    /// with no sharing, so a sharing or lock violation shows another program holds it.
    fn open(&self, name: &str, exclusive: bool) -> io::Result<File>;
    /// macOS: whether the folder is on a local APFS or HFS+ volume (`MNT_LOCAL`, and `apfs` or
    /// `hfs` as its type); `None` when it can't be read. Asked only by the macOS stale rule.
    fn local_apfs_or_hfs(&self) -> Option<bool>;
}

/// A folder looked at by its path: for the app's own matter paths (undo, restore, the
/// writeback), where no agent names the path.
pub(crate) struct PathFolder<P: AsRef<Path>>(pub(crate) P);

impl<P: AsRef<Path>> DocumentFolder for PathFolder<P> {
    fn has(&self, name: &str) -> bool {
        self.0.as_ref().join(name).symlink_metadata().is_ok()
    }

    fn names(&self, max: usize) -> Option<Vec<String>> {
        Some(
            std::fs::read_dir(self.0.as_ref())
                .ok()?
                .take(max)
                .flatten()
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect(),
        )
    }

    fn open(&self, name: &str, exclusive: bool) -> io::Result<File> {
        let path = self.0.as_ref().join(name);
        #[cfg(windows)]
        if exclusive {
            return crate::write::open_exclusive(&path);
        }
        let _ = exclusive;
        File::open(path)
    }

    fn local_apfs_or_hfs(&self) -> Option<bool> {
        #[cfg(target_os = "macos")]
        {
            use nix::mount::MntFlags;
            let stat = nix::sys::statfs::statfs(self.0.as_ref()).ok()?;
            Some(
                stat.flags().contains(MntFlags::MNT_LOCAL)
                    && matches!(stat.filesystem_type_name(), "apfs" | "hfs"),
            )
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }
}
