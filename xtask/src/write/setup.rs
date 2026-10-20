//! What `setup --binaries` changes in cargo's bin: its staging dirs, its run lock, and the
//! binaries it installs there.

use std::fs::{self, File};
use std::io;
use std::path::Path;

/// Makes `bin` if needed, and `staging` in it, empty: whatever was at `staging` is removed first.
pub(crate) fn reset_staging_dir(bin: &Path, staging: &Path) -> io::Result<()> {
    fs::create_dir_all(bin).and_then(|()| match fs::remove_dir_all(staging) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => fs::create_dir(staging),
    })
}

/// Removes the staging dir `staging` and everything in it.
pub(crate) fn remove_staging_dir(staging: &Path) -> io::Result<()> {
    fs::remove_dir_all(staging)
}

/// Removes every entry in `bin` whose name starts with `prefix`. A symlink is removed, never
/// followed.
pub(crate) fn remove_staging_leftovers(bin: &Path, prefix: &str) -> io::Result<()> {
    for entry in fs::read_dir(bin)? {
        let entry = entry?;
        if !entry.file_name().to_string_lossy().starts_with(prefix) {
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

/// Makes `bin` if needed, then opens the run lock `name` in it. The file is opened for reading,
/// so one the user cannot write still locks. It is created only when it is missing.
pub(crate) fn open_run_lock(bin: &Path, name: &str) -> io::Result<File> {
    fs::create_dir_all(bin)?;
    let path = bin.join(name);
    File::open(&path).or_else(|_| File::create(&path))
}

/// Makes `unpacked` executable (on Unix), then moves it to `destination`, replacing what is
/// there.
pub(crate) fn install_executable(unpacked: &Path, destination: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(unpacked, fs::Permissions::from_mode(0o755))?;
    }
    fs::rename(unpacked, destination)
}
