//! The file changes the tests make: a scratch folder of their own, and planted files. The only
//! place in these tests that may call std's writes.

#![expect(
    clippy::disallowed_methods,
    reason = "the tests' scratch folder and planted files; clippy.toml bans std's writes elsewhere"
)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);

/// A folder of this test's own under the temp folder, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Scratch {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir =
            std::env::temp_dir().join(format!("clauseo-counts-{name}-{}-{n}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// Removes the file at `path`.
pub(crate) fn remove(path: &Path) {
    std::fs::remove_file(path).unwrap();
}

/// Writes `contents` at `path`, making its folders.
pub(crate) fn plant(path: &Path, contents: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

/// Makes `link` (which must not exist; its folder is made) a symbolic link to the folder
/// `target`.
#[cfg(unix)]
pub(crate) fn link_dir(link: &Path, target: &Path) {
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(target, link).unwrap();
}

/// Makes the folder at `path` read-only (`0555`), or writable again (`0755`), so a delete in
/// it fails.
#[cfg(unix)]
pub(crate) fn set_read_only(path: &Path, read_only: bool) {
    use std::os::unix::fs::PermissionsExt as _;
    let mode = if read_only { 0o555 } else { 0o755 };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Whether a file can be made in the folder at `path` (it can when the tests run as root, which
/// ignores the folder's mode).
#[cfg(unix)]
pub(crate) fn can_write_in(path: &Path) -> bool {
    let probe = path.join("probe");
    let made = std::fs::write(&probe, b"").is_ok();
    drop(std::fs::remove_file(&probe));
    made
}
