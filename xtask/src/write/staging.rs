//! A staging dir in a bin folder (cargo's bin, for `setup --binaries`): downloads go in it, and a
//! binary moves from it into bin. Each operation stays inside bin and the staging dir it made.
//! The staging dir's name is the caller's.

use super::{plain_name, sys};
use std::io;
use std::path::{Path, PathBuf};

/// A staging dir `<bin>/<name>` that this run made empty. Downloads go in it, and the binary
/// moves from it into bin, a rename on one filesystem.
#[derive(Debug)]
pub(crate) struct Staging {
    bin: PathBuf,
    dir: PathBuf,
}

impl Staging {
    /// Makes `bin` if needed, and `<bin>/<name>` in it, empty: whatever was there is removed
    /// first. Refuses a `name` that is not one plain file name.
    pub(crate) fn fresh(bin: &Path, name: &str) -> io::Result<Staging> {
        plain_name(name, "staging dir name")?;
        let dir = bin.join(name);
        sys::create_dir_all(bin)?;
        match sys::remove_dir_all(&dir) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => sys::create_dir(&dir)?,
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
            sys::set_permissions(file, std::fs::Permissions::from_mode(0o755))?;
        }
        sys::rename(file, &self.bin.join(name))
    }

    /// Removes the staging dir and everything in it. A dir that is already gone is fine.
    pub(crate) fn remove(&self) -> io::Result<()> {
        match sys::remove_dir_all(&self.dir) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

#[cfg(test)]
mod tests;
