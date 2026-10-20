//! Lock files: a file whose exclusive lock one run holds, so a second run of the same tool sees it.
//! The OS drops the lock when the process ends, killed or not. The door hands out a `LockFile`,
//! never a `File`: nothing can be written through it.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::{Component, Path};

/// The folder of xtask's lock files, under the repo root.
pub(crate) const LOCK_DIR: &str = "target/xtask";

/// An open lock file. Only its lock can be taken; it holds as long as the value lives.
#[derive(Debug)]
pub(crate) struct LockFile(File);

impl LockFile {
    /// Takes the exclusive lock, or says why not at once.
    pub(crate) fn try_lock(&self) -> Result<(), TryLockError> {
        self.0.try_lock()
    }

    /// Takes the exclusive lock, waiting while another handle holds it.
    pub(crate) fn lock(&self) -> io::Result<()> {
        self.0.lock()
    }

    /// Wraps a file the write door opened as a lock file.
    pub(super) fn of(file: File) -> LockFile {
        LockFile(file)
    }
}

/// Opens the lock file `<root>/target/xtask/<name>` for reading and writing, creating it and
/// its folders when missing. An existing file is not truncated. Refuses a `name` that is not a
/// plain `<name>.lock` file name.
pub(crate) fn create_lock_file(root: &Path, name: &str) -> io::Result<LockFile> {
    let dir = root.join(LOCK_DIR);
    let path = dir.join(lock_name(name)?);
    std::fs::create_dir_all(&dir)?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
        .map(LockFile)
}

/// Opens the existing lock file `<root>/target/xtask/<name>` for reading and writing, to test its
/// lock. A missing file is never created: the open fails with `NotFound`. Refuses a `name`
/// that is not a plain `<name>.lock` file name.
pub(crate) fn open_lock_file(root: &Path, name: &str) -> io::Result<LockFile> {
    let path = root.join(LOCK_DIR).join(lock_name(name)?);
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map(LockFile)
}

/// `name` when it is one plain file name ending in `.lock`, else the refusal.
fn lock_name(name: &str) -> io::Result<&str> {
    let mut parts = Path::new(name).components();
    let plain = matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    );
    if plain && Path::new(name).extension().is_some_and(|ext| ext == "lock") && !name.contains('\\')
    {
        Ok(name)
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "`{name}` is not a lock file name: a lock file is a plain <name>.lock in {LOCK_DIR}"
            ),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;

    #[test]
    fn a_lock_file_is_a_plain_name_in_target_xtask() {
        let dir = Scratch::new("write-lock-names");
        for name in [
            "../Cargo.toml",
            "Cargo.toml",
            "sub/a.lock",
            ".lock",
            "/tmp/a.lock",
        ] {
            let refused = create_lock_file(&dir, name).expect_err("refused");
            assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(
                refused.to_string(),
                format!(
                    "`{name}` is not a lock file name: a lock file is a plain <name>.lock in target/xtask"
                )
            );
            assert_eq!(
                open_lock_file(&dir, name).expect_err("refused").to_string(),
                refused.to_string()
            );
        }
        assert!(!dir.join("target").exists(), "nothing was made");
        let lock = create_lock_file(&dir, "a.lock").expect("made");
        lock.try_lock().expect("the lock is free");
        assert!(dir.join("target/xtask/a.lock").is_file());
    }
}
