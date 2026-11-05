//! The tests' scratch folders and the std calls that change files, as thin wrappers: the one
//! place this crate's unit tests change files. Workspace `clippy.toml` bans each call, tests
//! included, and the expect below covers only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the unit tests' file setup calls std's writes here alone; clippy.toml bans them elsewhere"
)]

use std::path::{Path, PathBuf};

/// A folder under `std::env::temp_dir()` that one test owns; removed on drop. Its path is not
/// canonical (on macOS it starts `/var`, which is `/private/var`), on purpose.
pub(crate) struct TempDir(PathBuf);

impl TempDir {
    /// A fresh, empty folder named for `test` and this process.
    pub(crate) fn new(test: &str) -> TempDir {
        let path =
            std::env::temp_dir().join(format!("clauseo-sandbox-{test}-{}", std::process::id()));
        if path.exists() {
            std::fs::remove_dir_all(&path).unwrap();
        }
        std::fs::create_dir_all(&path).unwrap();
        TempDir(path)
    }

    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }

    /// `rel` under the folder, made as a folder (with its parents).
    pub(crate) fn dir(&self, rel: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// `rel` under the folder, written with `content` (parents made).
    pub(crate) fn file(&self, rel: &str, content: &str) -> PathBuf {
        let path = self.0.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(&path, content).unwrap();
        path
    }

    /// A symbolic link at `rel` under the folder, pointing at `target`.
    #[cfg(unix)]
    pub(crate) fn symlink(&self, rel: &str, target: &Path) -> PathBuf {
        let path = self.0.join(rel);
        std::os::unix::fs::symlink(target, &path).unwrap();
        path
    }

    /// A directory symbolic link at `rel` under the folder, pointing at `target`. An error when
    /// this process may not make symlinks (no admin and no Developer Mode).
    #[cfg(windows)]
    pub(crate) fn symlink_dir(&self, rel: &str, target: &Path) -> std::io::Result<PathBuf> {
        let path = self.0.join(rel);
        std::os::windows::fs::symlink_dir(target, &path)?;
        Ok(path)
    }

    /// A hard link at `rel` under the folder to the existing file `original`.
    pub(crate) fn hard_link(&self, rel: &str, original: &Path) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::hard_link(original, &path).unwrap();
        path
    }
}

/// Sets the permission bits of `path` to `mode`.
#[cfg(unix)]
pub(crate) fn set_mode(path: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Removes the file at `path`.
#[cfg(unix)]
pub(crate) fn remove(path: &Path) {
    std::fs::remove_file(path).unwrap();
}

impl Drop for TempDir {
    fn drop(&mut self) {
        // Never panic in a drop that may run while a test unwinds.
        if let Err(error) = std::fs::remove_dir_all(&self.0) {
            eprintln!("removing {}: {error}", self.0.display());
        }
    }
}
