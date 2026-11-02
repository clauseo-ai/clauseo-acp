//! The std calls that change files: the one place in `connectors` that may call them. The
//! workspace `clippy.toml` bans each of them everywhere else, and the expect below covers only this
//! file.
//!
//! One file changes: the state file, `<data_dir>/connectors/state.json` ([`replace_atomic`]).
//! With `test-support`, [`ScratchDir`] gives a test a folder of its own.

#![expect(
    clippy::disallowed_methods,
    reason = "connectors writes files here alone; clippy.toml bans std's writes elsewhere"
)]

use std::fs::OpenOptions;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Numbers the temp files of one process, so two calls never pick the same name.
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

/// Makes `target` hold exactly `bytes`: its folder is made if missing, then a new temp file
/// beside it, named for this process and this call, is written, synced and renamed over
/// `target`. On any error the temp file is removed and `target` is as it was.
pub(crate) fn replace_atomic(target: &Path, bytes: &[u8]) -> io::Result<()> {
    let dir = target
        .parent()
        .ok_or_else(|| io::Error::from(io::ErrorKind::InvalidInput))?;
    if !dir.as_os_str().is_empty() {
        std::fs::create_dir_all(dir)?;
    }
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    let temp = Temp(Some(
        dir.join(format!(".{name}.{}-{n}.tmp", std::process::id())),
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(temp.path())?;
    file.write_all(bytes).and_then(|()| file.sync_all())?;
    drop(file);
    temp.rename_to(target)
}

/// A temp file, removed on drop unless it was renamed.
struct Temp(Option<PathBuf>);

impl Temp {
    fn path(&self) -> &Path {
        self.0.as_deref().unwrap_or_else(|| Path::new(""))
    }

    /// Renames the temp file over `target`.
    fn rename_to(mut self, target: &Path) -> io::Result<()> {
        let Some(temp) = self.0.take() else {
            return Ok(());
        };
        if let Err(err) = std::fs::rename(&temp, target) {
            self.0 = Some(temp);
            return Err(err);
        }
        Ok(())
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        if let Some(temp) = self.0.take()
            && let Err(err) = std::fs::remove_file(&temp)
            && err.kind() != io::ErrorKind::NotFound
        {
            tracing::warn!(path = %temp.display(), error = %err, "temp file left behind");
        }
    }
}

/// A folder of a test's own under the OS temp folder, removed with everything in it on drop.
#[cfg(feature = "test-support")]
#[derive(Debug)]
pub struct ScratchDir(PathBuf);

#[cfg(feature = "test-support")]
impl ScratchDir {
    /// A new, empty folder named for `name`, this process and this call.
    pub fn new(name: &str) -> io::Result<ScratchDir> {
        let n = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "clauseo-connectors-{name}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir)?;
        Ok(ScratchDir(dir))
    }

    /// The folder.
    pub fn path(&self) -> &Path {
        &self.0
    }

    /// `<folder>/connectors/state.json`, not yet written.
    pub fn state_file(&self) -> PathBuf {
        self.0.join("connectors").join("state.json")
    }

    /// Writes `bytes` to `rel` under the folder (one component), for a test's own setup.
    pub fn write(&self, rel: &str, bytes: &[u8]) -> io::Result<PathBuf> {
        let path = self.0.join(rel);
        std::fs::write(&path, bytes)?;
        Ok(path)
    }
}

#[cfg(feature = "test-support")]
impl Drop for ScratchDir {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

#[cfg(test)]
#[cfg(feature = "test-support")]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn replace_makes_the_folder_and_leaves_no_temp() {
        let scratch = ScratchDir::new("replace").unwrap();
        let target = scratch.state_file();
        replace_atomic(&target, b"one").unwrap();
        replace_atomic(&target, b"two, longer").unwrap();
        assert_eq!(std::fs::read(&target).unwrap(), b"two, longer");
        assert_eq!(names(target.parent().unwrap()), ["state.json"]);
    }

    #[test]
    fn a_folder_at_the_target_fails_and_leaves_no_temp() {
        let scratch = ScratchDir::new("folder").unwrap();
        let target = scratch.path().join("state.json");
        std::fs::create_dir_all(target.join("inside")).unwrap();
        replace_atomic(&target, b"new").unwrap_err();
        assert_eq!(names(scratch.path()), ["state.json"]);
    }
}
