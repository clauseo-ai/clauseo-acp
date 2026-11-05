//! The ledger's file changes: its folder, made once, and the whole-file replace. The one place
//! the ledger changes files; workspace `clippy.toml` bans each call, and the expect below covers
//! only this file.

#![expect(
    clippy::disallowed_methods,
    reason = "the ACL ledger's folder and its atomic replace happen here alone"
)]

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Replaces `path` with `bytes`: see [`replace_with`].
pub(super) fn replace(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace_with(path, bytes, Write::write_all)
}

/// Replaces `path` with `bytes` so that a reader sees the old file or the new one, never part of
/// either: the bytes go to a temporary file beside it, made new (never through a link left
/// there), through `write`, then are flushed to disk, then the temporary file is renamed over
/// `path`. When any step fails, the temporary file is removed and `path` is as it was.
///
/// `path`'s folder (`<data_dir>\sandbox`) is made when missing; its parent must exist. A link
/// or a file standing there is refused. Tests pass a `write` that fails partway.
pub(super) fn replace_with(
    path: &Path,
    bytes: &[u8],
    write: impl FnOnce(&mut File, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let folder = path
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the ledger has no folder"))?;
    make_folder(folder)?;
    let temp = temp_path(path)?;
    let mut file = create_new(&temp)?;
    let written = write(&mut file, bytes).and_then(|()| file.sync_all());
    drop(file);
    if let Err(error) = written.and_then(|()| std::fs::rename(&temp, path)) {
        if let Err(cleanup) = std::fs::remove_file(&temp) {
            let cleanup = cleanup.kind();
            tracing::debug!(%cleanup, "the ledger's temporary file wasn't removed");
        }
        return Err(error);
    }
    Ok(())
}

/// `.acl-ledger.json.tmp` beside `path`.
fn temp_path(path: &Path) -> io::Result<PathBuf> {
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "the ledger has no name"))?;
    let mut temp = std::ffi::OsString::from(".");
    temp.push(name);
    temp.push(".tmp");
    Ok(path.with_file_name(temp))
}

/// Makes `temp` new. One left by a crash is removed first (a link itself, never its target).
fn create_new(temp: &Path) -> io::Result<File> {
    let open = || OpenOptions::new().write(true).create_new(true).open(temp);
    match open() {
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            std::fs::remove_file(temp)?;
            open()
        }
        made => made,
    }
}

/// Makes `folder` when missing (one level), and checks it is a real folder, not a link.
fn make_folder(folder: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(folder) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match std::fs::create_dir(folder) {
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                made => made?,
            }
        }
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    let meta = std::fs::symlink_metadata(folder)?;
    if meta.is_dir() && !meta.file_type().is_symlink() {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a link or a file stands where the ledger's folder goes",
        ))
    }
}
