//! Scratch directories for tests that touch real files. Each lives under the temp dir and is
//! removed when it drops, so a test that fails half-way leaves nothing behind.

use std::fs;
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// An empty directory `xtask-buildtime-<name>-<pid>` under the temp dir, removed on drop.
pub(crate) struct Scratch(PathBuf);

impl Scratch {
    pub(crate) fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("xtask-buildtime-{name}-{}", std::process::id()));
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
