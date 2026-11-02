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

/// Writes `contents` at `path`, making its folders.
pub(crate) fn plant(path: &Path, contents: &[u8]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}
