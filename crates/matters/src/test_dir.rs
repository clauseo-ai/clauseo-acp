//! A folder under `std::env::temp_dir()` for one unit test, named from the test and removed when
//! dropped.

use std::path::{Path, PathBuf};

use crate::write;

/// A test's own folder, empty when made, removed on drop.
pub(crate) struct TestDir {
    path: PathBuf,
}

impl TestDir {
    /// `<temp>/clauseo-matters-unit-<name>-<pid>`, emptied first if a crashed run left it.
    pub(crate) fn new(name: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "clauseo-matters-unit-{name}-{}",
            std::process::id()
        ));
        // A leftover from an earlier run may or may not exist.
        if let Err(err) = write::remove_dir_all(&path) {
            assert_eq!(err.kind(), std::io::ErrorKind::NotFound, "{err}");
        }
        write::create_dir_all(&path).unwrap();
        // Canonical, so tests compare paths as the code sees them (macOS: /private/var).
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
        if let Err(err) = write::remove_dir_all(&self.path) {
            tracing::warn!(path = %self.path.display(), error = %err, "test folder left behind");
        }
    }
}
