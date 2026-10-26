//! The integration tests' helpers: a folder per test, the std writes the tests need, a probe
//! that marks chosen files cloud-only, and settings built around it.

#![expect(
    clippy::disallowed_methods,
    reason = "the matters integration tests' file setup calls std's writes here alone; \
              clippy.toml bans them elsewhere"
)]

use std::collections::BTreeSet;
use std::fs::{File, FileTimes};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use clauseo_matters::{Limits, PlaceholderProbe, Retention, Settings};

/// A test's own folder, empty when made, removed on drop.
pub(crate) struct TestDir {
    path: PathBuf,
}

impl TestDir {
    /// `<temp>/clauseo-matters-it-<name>-<pid>`, emptied first if a crashed run left it, and
    /// canonical (macOS: `/private/var/...`).
    pub(crate) fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("clauseo-matters-it-{name}-{}", std::process::id()));
        if let Err(err) = std::fs::remove_dir_all(&path) {
            assert_eq!(err.kind(), io::ErrorKind::NotFound, "{err}");
        }
        std::fs::create_dir_all(&path).unwrap();
        let path = dunce::canonicalize(&path).unwrap();
        Self { path }
    }

    /// The folder.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TestDir {
    fn drop(&mut self) {
        if let Err(err) = std::fs::remove_dir_all(&self.path) {
            tracing::warn!(path = %self.path.display(), error = %err, "test folder left behind");
        }
    }
}

/// `fs::write`, after creating the parent folders.
pub(crate) fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
    let path = path.as_ref();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, contents).unwrap();
}

/// `fs::create_dir_all`.
pub(crate) fn create_dir_all(path: impl AsRef<Path>) {
    std::fs::create_dir_all(path).unwrap();
}

/// `fs::remove_file`.
pub(crate) fn remove_file(path: impl AsRef<Path>) {
    std::fs::remove_file(path).unwrap();
}

/// `std::os::unix::fs::symlink`.
#[cfg(unix)]
pub(crate) fn symlink(target: impl AsRef<Path>, link: impl AsRef<Path>) {
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// `fs::rename`.
pub(crate) fn rename(from: impl AsRef<Path>, to: impl AsRef<Path>) {
    std::fs::rename(from, to).unwrap();
}

/// Sets a file's Unix permission bits; mode `0o000` makes any read of it fail.
#[cfg(unix)]
pub(crate) fn set_mode(path: impl AsRef<Path>, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Sets the file's modification time with `File::set_times`.
pub(crate) fn set_mtime(path: impl AsRef<Path>, mtime: SystemTime) {
    let file = File::options().write(true).open(path).unwrap();
    file.set_times(FileTimes::new().set_modified(mtime))
        .unwrap();
}

/// A probe that answers cloud-only for the absolute paths a test marks; marks can change
/// between calls.
#[derive(Debug, Default)]
pub(crate) struct MarkedProbe {
    marked: Mutex<BTreeSet<PathBuf>>,
}

impl MarkedProbe {
    /// Marks `path` (absolute) cloud-only.
    pub(crate) fn mark(&self, path: impl Into<PathBuf>) {
        self.marked.lock().unwrap().insert(path.into());
    }

    /// Unmarks `path`.
    pub(crate) fn unmark(&self, path: impl AsRef<Path>) {
        self.marked.lock().unwrap().remove(path.as_ref());
    }
}

impl PlaceholderProbe for MarkedProbe {
    fn is_cloud_only(&self, path: &Path, _metadata: &std::fs::Metadata) -> bool {
        self.marked.lock().unwrap().contains(path)
    }
}

/// Settings with `root` as the matters root, [`Limits::DEFAULT`], [`Retention::KeepAll`] and
/// `probe`.
pub(crate) fn settings(root: impl Into<PathBuf>, probe: Arc<MarkedProbe>) -> Settings {
    Settings {
        matters_root: root.into(),
        limits: Limits::DEFAULT,
        retention: Retention::KeepAll,
        probe,
    }
}
