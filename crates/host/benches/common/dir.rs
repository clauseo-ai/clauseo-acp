//! The bench's scratch folders: `temp_dir()/clauseo-host-bench-<name>-<pid>`, removed on drop,
//! with the fixture store's copies made under it. The benches' one place that creates, copies
//! and removes folders; clippy.toml bans std's writes elsewhere.

#![expect(
    clippy::disallowed_methods,
    reason = "the benches make, copy and remove their scratch folders here alone"
)]

use std::io;
use std::path::{Path, PathBuf};

/// The files of a store folder that a copy carries: `state.sqlite` and SQLite's `-wal` and
/// `-shm`, when they exist. The lock file is not one of them: the host takes its own.
const STORE_FILES: [&str; 3] = ["state.sqlite", "state.sqlite-wal", "state.sqlite-shm"];

pub(crate) struct BenchDir(PathBuf);

impl BenchDir {
    pub(crate) fn new(name: &str) -> io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("clauseo-host-bench-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&path));
        std::fs::create_dir_all(&path)?;
        Ok(Self(path))
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for BenchDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

/// Makes `path` and its parents.
pub(crate) fn make(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Makes `to` and copies into it the store files `from` holds.
pub(crate) fn copy_store(from: &Path, to: &Path) -> io::Result<()> {
    make(to)?;
    for name in STORE_FILES {
        let source = from.join(name);
        if source.exists() {
            std::fs::copy(&source, to.join(name))?;
        }
    }
    Ok(())
}

/// Removes `path` and everything in it.
pub(crate) fn remove(path: &Path) -> io::Result<()> {
    std::fs::remove_dir_all(path)
}
