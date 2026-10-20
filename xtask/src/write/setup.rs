//! What `setup --binaries` changes in cargo's bin: its staging dirs, its run lock, and the
//! binaries it installs there. Each operation stays inside bin and the staging dir it made.

use super::lock::LockFile;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Component, Path, PathBuf};

/// The start of the name of every staging directory `setup` makes in bin.
pub(crate) const STAGING_PREFIX: &str = ".xtask-setup-";

/// The file in bin whose exclusive lock `setup --binaries` holds for the whole run, so one run at a
/// time uses bin. It is never deleted, and `STAGING_PREFIX` does not match it.
pub(crate) const RUN_LOCK: &str = ".xtask-setup.lock";

/// A staging dir `<bin>/.xtask-setup-<label>` that this run made empty. Downloads go in it, and
/// the binary moves from it into bin, a rename on one filesystem.
#[derive(Debug)]
pub(crate) struct Staging {
    bin: PathBuf,
    dir: PathBuf,
}

impl Staging {
    /// Makes `bin` if needed, and `<bin>/.xtask-setup-<label>` in it, empty: whatever was there
    /// is removed first. Refuses a `label` that is not one plain file name.
    pub(crate) fn fresh(bin: &Path, label: &str) -> io::Result<Staging> {
        plain_name(label, "staging label")?;
        let dir = bin.join(format!("{STAGING_PREFIX}{label}"));
        fs::create_dir_all(bin)?;
        match fs::remove_dir_all(&dir) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => fs::create_dir(&dir)?,
        }
        Ok(Staging {
            bin: bin.to_path_buf(),
            dir,
        })
    }

    /// The staging dir.
    pub(crate) fn dir(&self) -> &Path {
        &self.dir
    }

    /// Makes `file`, a file directly in the staging dir, executable (on Unix), then moves it to
    /// `<bin>/<name>`, replacing what is there. Refuses any other `file`, and a `name` that is not
    /// one plain file name.
    pub(crate) fn install(&self, file: &Path, name: &str) -> io::Result<()> {
        plain_name(name, "binary name")?;
        if file.parent() != Some(self.dir.as_path()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "{} is not directly in the staging dir {}, so it is not installed",
                    file.display(),
                    self.dir.display()
                ),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(file, fs::Permissions::from_mode(0o755))?;
        }
        fs::rename(file, self.bin.join(name))
    }

    /// Removes the staging dir and everything in it. A dir that is already gone is fine.
    pub(crate) fn remove(&self) -> io::Result<()> {
        match fs::remove_dir_all(&self.dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

/// Removes every entry in `bin` whose name starts with `STAGING_PREFIX`. A symlink is removed,
/// never followed.
pub(crate) fn remove_staging_leftovers(bin: &Path) -> io::Result<()> {
    for entry in fs::read_dir(bin)? {
        let entry = entry?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(STAGING_PREFIX)
        {
            continue;
        }
        let path = entry.path();
        if fs::symlink_metadata(&path)?.is_dir() {
            fs::remove_dir_all(&path)?;
        } else {
            fs::remove_file(&path)?;
        }
    }
    Ok(())
}

/// Makes `bin` if needed, then opens `<bin>/.xtask-setup.lock`. An existing file is opened for
/// reading, so one the user cannot write still locks. A missing one is created, empty; an open
/// that fails for any other reason fails, and nothing is truncated.
pub(crate) fn open_run_lock(bin: &Path) -> io::Result<LockFile> {
    fs::create_dir_all(bin)?;
    let path = bin.join(RUN_LOCK);
    match File::open(&path) {
        Ok(file) => Ok(LockFile::of(file)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map(LockFile::of),
        Err(error) => Err(error),
    }
}

/// `Ok` when `name` is one plain file name, else the refusal naming `what` it is.
fn plain_name(name: &str, what: &str) -> io::Result<()> {
    let mut parts = Path::new(name).components();
    if matches!(
        (parts.next(), parts.next()),
        (Some(Component::Normal(_)), None)
    ) && !name.contains('\\')
    {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("`{name}` is not a {what}: it must be one plain file name"),
        ))
    }
}

#[cfg(test)]
mod tests;
