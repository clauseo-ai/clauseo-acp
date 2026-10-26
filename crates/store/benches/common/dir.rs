//! A fresh folder for one bench, `temp_dir()/clauseo-store-bench-<name>-<pid>`, removed on drop.
//! The benches' one place that creates and removes folders; clippy.toml bans std's writes
//! elsewhere.

#![expect(
    clippy::disallowed_methods,
    reason = "the benches make and remove their scratch folders here alone"
)]

use std::path::{Path, PathBuf};

pub(crate) struct BenchDir(PathBuf);

impl BenchDir {
    pub(crate) fn new(name: &str) -> std::io::Result<Self> {
        let path =
            std::env::temp_dir().join(format!("clauseo-store-bench-{name}-{}", std::process::id()));
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
