//! Scratch directories for tests that touch real files, for any check or command in xtask. Each
//! lives under the temp dir and is removed when it drops, so a test that fails half-way leaves
//! nothing behind.

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// An empty directory `xtask-<name>-<pid>` under the temp dir, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    /// Makes the directory, clearing what a killed earlier run left under the same name.
    ///
    /// `name` must be unique per test, across all of xtask's tests. `cargo test` runs tests as
    /// threads of one process, so two tests sharing a name would share a pid too, and each would
    /// wipe the other's directory. Start it with the module, e.g. `buildtime-stray`.
    pub(crate) fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
        if dir.exists() {
            fs::remove_dir_all(&dir).expect("clear the old scratch dir");
        }
        fs::create_dir_all(&dir).expect("create the scratch dir");
        Scratch(dir)
    }
}

impl Deref for Scratch {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Err(error) = fs::remove_dir_all(&self.0) {
            eprintln!("could not remove {}: {error}", self.0.display());
        }
    }
}
