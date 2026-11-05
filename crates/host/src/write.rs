//! The host's file system writes: the one module that may call std's write functions.

#![expect(
    clippy::disallowed_methods,
    reason = "the host's one place that creates folders (the data folder, the matters root and \
              agents' homes), writes an agent home's config.toml, appends to and rotates the log \
              file, and writes unit tests' temp folders"
)]

use std::io;
use std::path::Path;

/// Creates `path` and every missing parent; an existing folder is fine.
pub(crate) fn create_dir_all(path: &Path) -> io::Result<()> {
    std::fs::create_dir_all(path)
}

/// Creates the one folder `path`, private to the user on Unix (0700). Its parent must exist; a
/// link or anything else already at `path` fails with `AlreadyExists`, never followed.
pub(crate) fn create_private_dir(path: &Path) -> io::Result<()> {
    let builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    let builder = {
        let mut builder = builder;
        std::os::unix::fs::DirBuilderExt::mode(&mut builder, 0o700);
        builder
    };
    builder.create(path)
}

/// Creates `target` and every missing folder between `base` and it, each new one private to
/// the user on Unix (0700), through no link: each part below `base` must be a real folder, or be
/// made as one (`symlink_metadata`, which never follows a link, and on Windows counts a
/// junction as a link). A link, or anything not a folder, below `base` fails with
/// `InvalidInput`; `base` itself may be a link. A session's run folder and a matter's notes
/// folder (`actor/turn/launch_checks.rs`).
pub(crate) fn create_private_dirs_under(base: &Path, target: &Path) -> io::Result<()> {
    let below = target
        .strip_prefix(base)
        .map_err(|_outside| io::Error::new(io::ErrorKind::InvalidInput, "not under its base"))?;
    let mut at = base.to_owned();
    for part in below.components() {
        at.push(part);
        match std::fs::symlink_metadata(&at) {
            // Another launch may make it first: `AlreadyExists` is fine, and the check below
            // still refuses a link or a file there.
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                match create_private_dir(&at) {
                    Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                    made => made?,
                }
            }
            Err(error) => return Err(error),
            Ok(_) => {}
        }
        if !is_real_dir(&at) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("{} is a link or not a folder", at.display()),
            ));
        }
    }
    Ok(())
}

/// Whether `path` is a folder itself, not a link to one (nor, on Windows, a junction).
pub(crate) fn is_real_dir(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.is_dir())
}

/// Renames the folder `from` to `to` in one call: the start-up sweep's `run` set aside
/// (`actor/turn/launch_checks.rs`).
pub(crate) fn rename_folder(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Opens the file `path` for appending, creating it (0600 on Unix): the host's log file
/// (`log_file.rs`, decision record 0047).
pub(crate) fn open_append(path: &Path) -> io::Result<std::fs::File> {
    let mut options = std::fs::OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    options.open(path)
}

/// Renames the file `from` to `to`, replacing a file at `to`: the log file's rotation.
pub(crate) fn rename_file(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Removes the folder `path` and everything under it; a missing folder is fine. A link inside
/// is removed, never followed (`std::fs::remove_dir_all`): a session's run folder.
pub(crate) fn remove_tree(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Removes the link or file `path` itself, never what a link points to: a leftover download's
/// working entry that is not a real folder (`browser/install/archive.rs`). On Windows a link to
/// a folder (or a junction) is removed as a folder, which removes the link only.
pub(crate) fn remove_link(path: &Path) -> io::Result<()> {
    match std::fs::remove_file(path) {
        Err(error) if cfg!(windows) && std::fs::symlink_metadata(path).is_ok() => {
            std::fs::remove_dir(path).map_err(|_dir| error)
        }
        other => other,
    }
}

/// Sets the folder `path` to 0700 when it is not already (a link is left as it is).
#[cfg(unix)]
pub(crate) fn make_private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let meta = std::fs::symlink_metadata(path)?;
    if meta.is_dir() && meta.permissions().mode() & 0o777 != 0o700 {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

/// Replaces the file `path` with `bytes` atomically: a fresh temp file beside it (0600 on Unix),
/// written and synced, then renamed over `path`. A link at `path` is replaced, never followed.
/// The temp file's name holds the process id and a count, so two writes (or two hosts sharing a
/// data folder) never share one.
pub(crate) fn replace_private_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    use std::io::Write;
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut temp = path.as_os_str().to_owned();
    let count = COUNT.fetch_add(1, Ordering::Relaxed);
    temp.push(format!(".clauseo-{}-{count}.new", std::process::id()));
    let temp = std::path::PathBuf::from(temp);
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    std::os::unix::fs::OpenOptionsExt::mode(&mut options, 0o600);
    let written = options.open(&temp).and_then(|mut file| {
        file.write_all(bytes)?;
        file.sync_all()
    });
    let renamed = written.and_then(|()| std::fs::rename(&temp, path));
    if renamed.is_err() {
        // What is left of the temp file is ours; the error that matters is the write's.
        drop(std::fs::remove_file(&temp));
    }
    renamed
}

/// Removes the temp files a crashed [`replace_private_file`] of `path` left beside it
/// (`<name>.clauseo-*.new`), best effort: one that cannot be removed stays, and the write goes
/// on with its own fresh name.
pub(crate) fn sweep_temp_files(path: &Path) {
    let (Some(folder), Some(name)) = (path.parent(), path.file_name()) else {
        return;
    };
    let mut prefix = name.to_owned();
    prefix.push(".clauseo-");
    let prefix = prefix.as_encoded_bytes();
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let bytes = name.as_encoded_bytes();
        if bytes.starts_with(prefix) && bytes.ends_with(b".new") {
            drop(std::fs::remove_file(entry.path()));
        }
    }
}

/// Removes `path` and everything under it; a missing folder is fine. For unit tests' temp folders.
#[cfg(test)]
pub(crate) fn remove_dir_all(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        other => other,
    }
}

/// Writes `bytes` to the file `path`, replacing it. For unit tests' fixtures.
#[cfg(test)]
pub(crate) fn write_file(path: &Path, bytes: &[u8]) -> io::Result<()> {
    std::fs::write(path, bytes)
}

/// Renames `from` to `to`. For unit tests: the lawyer moving a folder.
#[cfg(test)]
pub(crate) fn rename(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::rename(from, to)
}

/// Removes the file `path`. For unit tests.
#[cfg(test)]
pub(crate) fn remove_file(path: &Path) -> io::Result<()> {
    std::fs::remove_file(path)
}

/// Sets `path`'s permission bits to `mode`. For unit tests: a folder the user may not search.
#[cfg(all(test, unix))]
pub(crate) fn set_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
}

/// Opens `path` for reading with a share mode that lets others read but not write or delete
/// (`FILE_SHARE_READ`), as Word holds a document. For the pre-write guard's Windows test.
#[cfg(all(test, windows))]
pub(crate) fn open_deny_write(path: &Path) -> io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    /// `FILE_SHARE_READ`.
    const SHARE_READ: u32 = 0x1;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(SHARE_READ)
        .open(path)
}

/// Makes `link` a symbolic link to `target`. For unit tests: a matter stored as a link.
#[cfg(all(test, unix))]
pub(crate) fn symlink(target: &Path, link: &Path) -> io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}
