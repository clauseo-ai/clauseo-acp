//! Lock files: a file whose exclusive lock one run holds, so a second run of the same tool sees it.
//! The OS drops the lock when the process ends, killed or not. The door hands out a `LockFile`,
//! never a `File`: nothing can be written through it.

use super::{plain_name, sys};
use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;

/// An open lock file. Only its lock can be taken; it holds as long as the value lives.
#[derive(Debug)]
pub(crate) struct LockFile(File);

impl LockFile {
    /// Makes `dir` if needed, then opens the lock file `<dir>/<name>`. An existing file is
    /// opened for reading, so one the user cannot write still locks; it is never truncated. A
    /// missing one is created, empty. An open that fails for any other reason fails. Refuses a
    /// `name` that is not one plain file name ending in `.lock`.
    pub(crate) fn open(dir: &Path, name: &str) -> io::Result<LockFile> {
        lock_name(name)?;
        sys::create_dir_all(dir)?;
        let path = dir.join(name);
        match File::open(&path) {
            Ok(file) => Ok(LockFile(file)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                let mut create = OpenOptions::new();
                create.write(true).create(true).truncate(false);
                sys::open(&create, &path).map(LockFile)
            }
            Err(error) => Err(error),
        }
    }

    /// Takes the exclusive lock, or says why not at once.
    pub(crate) fn try_lock(&self) -> Result<(), TryLockError> {
        self.0.try_lock()
    }

    /// Takes the exclusive lock, waiting while another handle holds it.
    pub(crate) fn lock(&self) -> io::Result<()> {
        self.0.lock()
    }
}

/// `Ok` when `name` is one plain file name ending in `.lock`, else the refusal.
fn lock_name(name: &str) -> io::Result<()> {
    let plain = plain_name(name, "lock file name").is_ok();
    if plain && Path::new(name).extension().is_some_and(|ext| ext == "lock") {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{name}` is not a lock file name: it must be one plain <name>.lock file name"),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scratch::Scratch;
    use std::fs;

    #[test]
    fn a_lock_file_is_a_plain_name_directly_in_its_folder() {
        let dir = Scratch::new("write-lock-names");
        let locks = dir.join("target/xtask");
        for name in [
            "../Cargo.toml",
            "Cargo.toml",
            "sub/a.lock",
            ".lock",
            "/tmp/a.lock",
            "a\\b.lock",
        ] {
            let refused = LockFile::open(&locks, name).expect_err("refused");
            assert_eq!(refused.kind(), io::ErrorKind::InvalidInput);
            assert_eq!(
                refused.to_string(),
                format!(
                    "`{name}` is not a lock file name: it must be one plain <name>.lock file name"
                )
            );
        }
        assert!(!dir.join("target").exists(), "nothing was made");
        let lock = LockFile::open(&locks, "a.lock").expect("made with its folders");
        lock.try_lock().expect("the lock is free");
        assert!(locks.join("a.lock").is_file());
        let second = LockFile::open(&locks, "a.lock").expect("opened again");
        assert!(matches!(second.try_lock(), Err(TryLockError::WouldBlock)));
        drop(lock);
        second.try_lock().expect("free once the first is dropped");
    }

    #[test]
    fn a_lock_file_is_created_when_missing_and_never_truncated() {
        let dir = Scratch::new("write-lock-kept");
        let bin = dir.join("bin");
        drop(LockFile::open(&bin, ".xtask-setup.lock").expect("created with bin"));
        Scratch::write(bin.join(".xtask-setup.lock"), "left by an earlier run").expect("bytes");
        drop(LockFile::open(&bin, ".xtask-setup.lock").expect("opened"));
        assert_eq!(
            fs::read_to_string(bin.join(".xtask-setup.lock")).expect("read it"),
            "left by an earlier run"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_lock_file_that_cannot_be_read_is_refused_and_never_truncated() {
        use std::os::unix::fs::PermissionsExt;
        let dir = Scratch::new("write-lock-unreadable");
        let lock = dir.join("a.lock");
        Scratch::write(&lock, "kept").expect("the lock file");
        // Write-only: the open for reading fails, the open for writing would succeed.
        Scratch::set_permissions(&lock, fs::Permissions::from_mode(0o200)).expect("chmod it");
        let refused = LockFile::open(&dir, "a.lock");
        Scratch::set_permissions(&lock, fs::Permissions::from_mode(0o644)).expect("chmod back");
        let error = refused.expect_err("a lock file that cannot be read is no lock");
        assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(fs::read_to_string(&lock).expect("read it"), "kept");
    }
}
